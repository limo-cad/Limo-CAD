//! One CAD view editor for presentation and printing. Draft previews never
//! change source geometry; saved views use the shared document mutation path.
use super::*;
use crate::session_bridge::parse_engine_envelope;
use limo_cad_core::{PrintBedDto, UnitSystem};
use limo_cad_interface::{ControlInput, Field as ControlField};
use limo_cad_sketch::{
    AssemblyDocumentDto, AssemblySolutionDto, AssemblyTransformDto, NamedViewConfigurationDto,
    NamedViewsDto, ViewCameraDto, ViewOccurrenceOffsetDto, ViewPartOffsetDto,
};
use workspace::DocumentReceipt;

mod panel;
pub(super) use panel::printer_choices;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    Saved,
    Name,
    Target,
    Translation(usize),
    Rotation(usize),
    PrintLayout,
    Printer,
    VisibleBody,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Field(Field),
    Create,
    Capture,
    Save,
    Rename,
    Delete,
    Recall,
    Reset,
    Preview,
    Check,
    ApplyCorrections,
    Scroll(i32),
    Close,
    Info,
    PrintSettings,
}

#[derive(Resource, Default)]
struct State {
    owner: Option<DocumentContext>,
    revision: Option<u64>,
    generation: u64,
    visible: bool,
    views: Vec<NamedViewConfigurationDto>,
    selected: Option<String>,
    original: Option<NamedViewConfigurationDto>,
    draft: Option<NamedViewConfigurationDto>,
    target: String,
    placement: Option<assembly::TransformDraft>,
    placement_dirty: bool,
    previewing: bool,
    preview_origin: Option<PreviewOrigin>,
    report: Option<Value>,
    error: Option<String>,
    scroll: usize,
    widgets: chrome::Widgets,
}

struct PreviewOrigin {
    receipt: DocumentReceipt,
    camera: native_viewport::ViewportCamera,
    presentation: native_viewport::ViewportPresentation,
}

pub(crate) fn active(world: &World) -> bool {
    world.get_resource::<State>().is_some_and(|s| s.visible)
}

pub(crate) fn cancel(
    world: &mut World,
    engine: &AppState,
    bridge: &SessionBridgeState,
    owner: &DocumentContext,
) -> Result<Value, String> {
    bridge.with_native_document_receipt(engine, owner, |revision| {
        let receipt = DocumentReceipt {
            owner: owner.clone(),
            revision,
        };
        let mut state = world
            .get_resource_mut::<State>()
            .ok_or("Open Named Views")?;
        if state.owner.as_ref() != Some(owner) {
            return Err("The named view belongs to a different document".into());
        }
        let origin = state
            .preview_origin
            .take()
            .filter(|origin| origin.receipt == receipt);
        state.visible = false;
        state.previewing = false;
        state.draft = state.original.clone();
        state.placement_dirty = false;
        state.placement = None;
        state.error = None;
        let restored = origin.is_some();
        if let Some(origin) = origin {
            native_viewport::apply_interface_view(
                world,
                &owner.document_id,
                Some(origin.camera),
                Some(origin.presentation),
            )?;
        }
        Ok(json!({"cancelled":true,"preview_restored":restored}))
    })
}

pub(crate) fn presentation_locked(world: &World) -> bool {
    feature::panel(world).is_some()
        || assembly::joint::active(world)
        || assembly::motion::active(world)
        || assembly::studies::active(world)
}

pub(crate) fn ensure_exportable(world: &World) -> Result<(), String> {
    if world.get_resource::<State>().is_some_and(|s| s.previewing) {
        Err(
            "Save and recall the previewed named view, or Reset to assembled placement before continuing"
                .into(),
        )
    } else {
        Ok(())
    }
}

/// Source feature forms consume source coordinates, never presentation picks.
pub(crate) fn ensure_source_ready(
    world: &mut World,
    engine: &AppState,
    bridge: &SessionBridgeState,
    owner: &DocumentContext,
) -> Result<(), String> {
    bridge.with_native_document_owner(engine, owner, || Ok(()))?;
    print_intent::ensure_clean(world)?;
    if world.get_resource::<State>().is_some_and(|s| s.previewing) {
        return Err("Save and recall the draft view, or Reset to assembled placement before editing source geometry".into());
    }
    if read_views(engine)?.active.is_some() {
        let (document, _, selection, _) = native_viewport::interface_view(world);
        if document != owner.document_id {
            return Err("The viewport is still changing documents".into());
        }
        let selected_bodies = selection.selected_body_ids.clone();
        let selected_occurrence = selection.selected_occurrence_id;
        let selected_faces = selection.selected_face_ids.clone();
        let selected_edges = selection.selected_edge_ids.clone();
        let selected_origin = selection.selected_origin_plane;
        let selected_datum = selection.selected_datum_plane_id;
        let result =
            bridge
                .apply_native_mutation(engine, owner, "clear_named_view", &json!({}), || Ok(()))?;
        let refreshed = finish_mutation(engine, bridge, world, "clear_named_view", result);
        if let Some(error) = refreshed["render_error"].as_str() {
            return Err(error.into());
        }
        let (_, _, mut presentation, _) = native_viewport::interface_view_snapshot(world);
        presentation.selected_body_ids = selected_bodies;
        presentation.selected_occurrence_id = selected_occurrence;
        presentation.selected_face_ids = selected_faces;
        presentation.selected_edge_ids = selected_edges;
        presentation.selected_origin_plane = selected_origin;
        presentation.selected_datum_plane_id = selected_datum;
        clear_pick_coordinates(&mut presentation);
        native_viewport::apply_interface_view(world, &owner.document_id, None, Some(presentation))?;
    }
    Ok(())
}

/// Retain the preview baseline across print-only edits that preserve its scene.
pub(crate) fn advance_metadata(world: &mut World, owner: &DocumentContext, revision: u64) {
    if let Some(mut state) = world
        .get_resource_mut::<State>()
        .filter(|s| s.owner.as_ref() == Some(owner))
    {
        let previous_revision = state.revision;
        if let Some(origin) = state.preview_origin.as_mut().filter(|origin| {
            origin.receipt.owner == *owner && Some(origin.receipt.revision) == previous_revision
        }) {
            origin.receipt.revision = revision;
        } else {
            state.preview_origin = None;
        }
        state.revision = Some(revision);
    }
}

pub(crate) fn after_metadata_history(world: &mut World, owner: &DocumentContext, revision: u64) {
    if let Some(mut state) = world.get_resource_mut::<State>().filter(|s| {
        s.owner.as_ref().is_some_and(|previous| {
            previous.window_id == owner.window_id && previous.document_id == owner.document_id
        }) && !s.previewing
    }) {
        state.preview_origin = None;
        state.owner = Some(owner.clone());
        state.revision = Some(revision);
        state.generation = state.generation.saturating_add(1);
    }
}

fn read_views(engine: &AppState) -> Result<NamedViewsDto, String> {
    serde_json::from_value(parse_engine_envelope(
        engine.engine_call("named_views", ""),
    )?)
    .map_err(|e| e.to_string())
}
fn read_assembly(engine: &AppState) -> Result<AssemblyDocumentDto, String> {
    serde_json::from_value(parse_engine_envelope(
        engine.engine_call("assembly_document", ""),
    )?)
    .map_err(|e| e.to_string())
}

pub(super) fn issue_message(engine: &AppState, issue: &Value) -> Result<String, String> {
    let mut message = issue["message"].as_str().unwrap_or("").to_string();
    if let Some(ids) = issue["occurrence_ids"]
        .as_array()
        .filter(|ids| !ids.is_empty())
    {
        let structure = read_assembly(engine)?.component_structure;
        let names: Vec<_> = ids
            .iter()
            .filter_map(Value::as_u64)
            .map(|id| {
                structure
                    .occurrences
                    .iter()
                    .find(|o| o.id.0 == id)
                    .map(|o| format!("{} (occurrence {id})", o.name))
                    .unwrap_or_else(|| format!("Occurrence {id}"))
            })
            .collect();
        message.push_str(&format!(" — {}", names.join(", ")));
    }
    Ok(message)
}

fn capture(
    world: &World,
    engine: &AppState,
    name: String,
) -> Result<NamedViewConfigurationDto, String> {
    let scene = engine.solid_scene_snapshot();
    capture_with_body_ids(
        world,
        engine,
        name,
        scene.bodies.iter().map(|body| body.id.0),
    )
}

fn capture_with_body_ids(
    world: &World,
    engine: &AppState,
    name: String,
    body_ids: impl Iterator<Item = u64>,
) -> Result<NamedViewConfigurationDto, String> {
    let (document, camera, presentation, _) = native_viewport::interface_view(world);
    if document != engine.active_project_session_id() {
        return Err("The viewport is still changing documents".into());
    }
    let saved = read_views(engine)?;
    let current = world
        .get_resource::<State>()
        .filter(|s| {
            s.previewing
                && s.owner
                    .as_ref()
                    .is_some_and(|owner| owner.document_id == document)
        })
        .and_then(|s| s.draft.clone())
        .or_else(|| {
            saved
                .active
                .as_ref()
                .and_then(|name| saved.views.iter().find(|v| &v.name == name))
                .cloned()
        });
    Ok(NamedViewConfigurationDto {
        id: None,
        name,
        camera: ViewCameraDto {
            position: camera.position.map(f64::from),
            target: camera.target.map(f64::from),
            up: camera.up.map(f64::from),
        },
        visible_body_ids: body_ids
            .filter(|id| !presentation.hidden_body_ids.contains(id))
            .collect(),
        part_offsets: current
            .as_ref()
            .map(|v| v.part_offsets.clone())
            .unwrap_or_default(),
        occurrence_offsets: current
            .as_ref()
            .map(|v| v.occurrence_offsets.clone())
            .unwrap_or_default(),
        print_layout: false,
        print_bed: current.map(|v| v.print_bed).unwrap_or_default(),
    })
}

pub(super) fn inspect(
    world: &World,
    engine: &AppState,
    body_ids: impl Iterator<Item = u64>,
) -> Result<Value, String> {
    let view = capture_with_body_ids(world, engine, String::new(), body_ids)?;
    let saved = read_views(engine)?;
    let workspace = workbench::workspace(world);
    let camera = if workspace == workbench::Workspace::Drawing {
        Value::Null
    } else {
        serde_json::to_value(view.camera).map_err(|error| error.to_string())?
    };
    Ok(json!({
        "camera":camera,
        "visible_body_ids":view.visible_body_ids,
        "part_offsets":view.part_offsets,
        "occurrence_offsets":view.occurrence_offsets,
        "active_named_view":saved.active,
        "mode":match workspace {
            workbench::Workspace::Solid => match native_viewport::interface_navigation_source(world).1.mode {
                native_viewport::ViewportMode::Solid => "solid",
                native_viewport::ViewportMode::PickPlane => "pick_plane",
                native_viewport::ViewportMode::Sketch => "sketch",
            },
            workbench::Workspace::Drawing => "drawing",
            workbench::Workspace::Cam => "cam",
        }
    }))
}

fn select(state: &mut State, view: NamedViewConfigurationDto, saved: bool) {
    state.selected = saved.then(|| view.name.clone());
    state.original = saved.then(|| view.clone());
    state.draft = Some(view);
    state.placement = None;
    state.target.clear();
    state.placement_dirty = false;
    state.report = None;
    state.error = None;
    state.scroll = 0;
    state.generation = state.generation.saturating_add(1);
}

pub(crate) fn open(
    world: &mut World,
    engine: &AppState,
    owner: &DocumentContext,
    name: Option<&str>,
) -> Result<Value, String> {
    if feature::panel(world).is_some() || assembly::joint::active(world) {
        return Err(
            "Apply or cancel the source feature or joint editor before editing named views".into(),
        );
    }
    if assembly::motion::active(world) || assembly::studies::active(world) {
        return Err("Stop and close the motion or study editor before editing named views".into());
    }
    if crate::native_editor::active(engine)?.is_some() {
        return Err("Finish the sketch before editing named views".into());
    }
    world.init_resource::<State>();
    let views = read_views(engine)?;
    let selected = name
        .or(views.active.as_deref())
        .and_then(|name| views.views.iter().find(|v| v.name == name))
        .cloned();
    let mut view = if let Some(view) = selected.clone() {
        view
    } else {
        capture(world, engine, "View 1".into())?
    };
    if selected.is_none() {
        let mut index = 1;
        while views.views.iter().any(|v| v.name == view.name) {
            index += 1;
            view.name = format!("View {index}");
        }
    }
    let mut state = world.resource_mut::<State>();
    if state.owner.as_ref() != Some(owner) {
        state.preview_origin = None;
        state.previewing = false;
        state.revision = None;
    }
    state.owner = Some(owner.clone());
    state.views = views.views;
    state.visible = true;
    select(&mut state, view, selected.is_some());
    Ok(json!({"named_views_visible":true}))
}

fn target_pose(view: &NamedViewConfigurationDto, target: &str) -> AssemblyTransformDto {
    if let Some(id) = target
        .strip_prefix("occurrence:")
        .and_then(|v| v.parse::<u64>().ok())
    {
        if let Some(offset) = view
            .occurrence_offsets
            .iter()
            .find(|o| o.occurrence_id.0 == id)
        {
            return AssemblyTransformDto {
                translation: offset.translation,
                rotation: offset.rotation,
            };
        }
    } else if let Some(id) = target
        .strip_prefix("body:")
        .and_then(|v| v.parse::<u64>().ok())
    {
        if let Some(offset) = view.part_offsets.iter().find(|o| o.body_id == id) {
            return AssemblyTransformDto {
                translation: offset.translation,
                ..default()
            };
        }
    }
    AssemblyTransformDto::default()
}

fn apply_placement(state: &mut State, units: UnitSystem) -> Result<(), String> {
    if !state.placement_dirty {
        return Ok(());
    }
    let pose = state
        .placement
        .as_ref()
        .ok_or("Choose a CAD occurrence or body")?
        .value(units)?;
    let view = state
        .draft
        .as_mut()
        .ok_or("Create or select a named view")?;
    if let Some(id) = state
        .target
        .strip_prefix("occurrence:")
        .and_then(|v| v.parse::<u64>().ok())
    {
        view.occurrence_offsets.retain(|o| o.occurrence_id.0 != id);
        view.occurrence_offsets.push(ViewOccurrenceOffsetDto {
            occurrence_id: limo_cad_sketch::OccurrenceId(id),
            translation: pose.translation,
            rotation: pose.rotation,
        });
    } else if let Some(id) = state
        .target
        .strip_prefix("body:")
        .and_then(|v| v.parse::<u64>().ok())
    {
        view.part_offsets.retain(|o| o.body_id != id);
        view.part_offsets.push(ViewPartOffsetDto {
            body_id: id,
            translation: pose.translation,
        });
    } else {
        return Err("Choose a CAD occurrence or body".into());
    }
    state.placement_dirty = false;
    state.report = None;
    Ok(())
}

fn apply_corrections(view: &mut NamedViewConfigurationDto, report: &Value) -> Result<(), String> {
    if report["proposal_fits"] != true {
        return Err("The proposed arrangement does not fit the selected bed".into());
    }
    let translations: Vec<limo_cad_export::LayoutTranslation> =
        serde_json::from_value(report["proposed_translations"].clone())
            .map_err(|e| e.to_string())?;
    if translations.is_empty() {
        return Err("No layout corrections are needed".into());
    }
    for proposed in translations {
        if let Some(offset) = view
            .occurrence_offsets
            .iter_mut()
            .find(|o| o.occurrence_id.0 == proposed.occurrence_id)
        {
            for axis in 0..3 {
                offset.translation[axis] += proposed.translation[axis];
            }
        } else {
            view.occurrence_offsets.push(ViewOccurrenceOffsetDto {
                occurrence_id: limo_cad_sketch::OccurrenceId(proposed.occurrence_id),
                translation: proposed.translation,
                rotation: [0., 0., 0., 1.],
            });
        }
    }
    Ok(())
}

fn apply_preview(
    world: &mut World,
    engine: &AppState,
    receipt: &DocumentReceipt,
    view: &NamedViewConfigurationDto,
    solution: AssemblySolutionDto,
) -> Result<(), String> {
    let (document, mut camera, mut presentation, _) =
        native_viewport::interface_view_snapshot(world);
    if document != receipt.owner.document_id {
        return Err("The viewport is still changing documents".into());
    }
    let origin = world
        .resource::<State>()
        .preview_origin
        .as_ref()
        .is_none_or(|origin| &origin.receipt != receipt)
        .then(|| PreviewOrigin {
            receipt: receipt.clone(),
            camera,
            presentation: presentation.clone(),
        });
    presentation.body_poses = solution.body_poses.into();
    presentation.instance_body_poses = solution.instance_body_poses.into();
    presentation.hidden_body_ids = engine
        .solid_scene_snapshot()
        .bodies
        .iter()
        .filter(|b| !view.visible_body_ids.contains(&b.id.0))
        .map(|b| b.id.0)
        .collect();
    clear_pick_coordinates(&mut presentation);
    camera.position = view.camera.position.map(|v| v as f32);
    camera.target = view.camera.target.map(|v| v as f32);
    camera.up = view.camera.up.map(|v| v as f32);
    native_viewport::apply_interface_view(world, &document, Some(camera), Some(presentation))?;
    let mut state = world.resource_mut::<State>();
    if let Some(origin) = origin {
        state.preview_origin = Some(origin);
    }
    state.previewing = true;
    Ok(())
}

pub(super) fn clear_pick_coordinates(presentation: &mut native_viewport::ViewportPresentation) {
    presentation.selected_surface_point = None;
    presentation.hovered_surface_point = None;
    presentation.sketch_point_support_plane = None;
    presentation.selected_sketch_points.clear();
    presentation.hovered_sketch_point = None;
}

pub(crate) fn after_mutation(
    world: &mut World,
    operation: &str,
    result: &Value,
) -> Result<(), String> {
    let was_previewing = world.get_resource::<State>().is_some_and(|s| s.previewing);
    if let Some(mut state) = world.get_resource_mut::<State>() {
        state.previewing = false;
        state.preview_origin = None;
    }
    let (document, mut camera, mut presentation, _) =
        native_viewport::interface_view_snapshot(world);
    if was_previewing
        || matches!(
            operation,
            "recall_named_view"
                | "clear_named_view"
                | "upsert_named_view"
                | "set_named_views"
                | "rename_named_view"
                | "delete_named_view"
        )
    {
        clear_pick_coordinates(&mut presentation);
    }
    if operation == "recall_named_view" {
        if let Some(view) = result.get("view") {
            let view: NamedViewConfigurationDto =
                serde_json::from_value(view.clone()).map_err(|e| e.to_string())?;
            camera.position = view.camera.position.map(|v| v as f32);
            camera.target = view.camera.target.map(|v| v as f32);
            camera.up = view.camera.up.map(|v| v as f32);
        }
    }
    native_viewport::apply_interface_view(world, &document, Some(camera), Some(presentation))?;
    Ok(())
}

fn saved_action_error(state: &State, command: &Command) -> Option<&'static str> {
    let message = match command {
        Command::Recall => "Save this draft or select a saved view before recalling it",
        Command::Rename => "Save this draft or select a saved view before renaming it",
        Command::Delete => "Save this draft or select a saved view before deleting it",
        _ => return None,
    };
    let saved = state.draft.is_some()
        && state.selected.as_ref().is_some_and(|name| {
            state
                .original
                .as_ref()
                .is_some_and(|view| &view.name == name)
                && state.views.iter().any(|view| &view.name == name)
        });
    (!saved).then_some(message)
}

pub(crate) fn reduce(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    engine: &AppState,
    bridge: &SessionBridgeState,
    action: &NativeInterfaceAction,
    generation: u64,
    command: &Command,
) -> Result<Value, String> {
    let result = reduce_inner(world, handle, engine, bridge, action, generation, command);
    if let Err(error) = &result {
        if let Some(mut state) = world.get_resource_mut::<State>().filter(|state| {
            state.visible
                && state.owner.as_ref() == Some(&action.context)
                && state.generation == generation
        }) {
            state.error = Some(error.clone());
        }
    }
    result
}

fn reduce_inner(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    engine: &AppState,
    bridge: &SessionBridgeState,
    action: &NativeInterfaceAction,
    generation: u64,
    command: &Command,
) -> Result<Value, String> {
    let receipt = bridge.native_document_receipt(engine, &action.context)?;
    handle.validate_action(action)?;
    if matches!(
        command,
        Command::Recall
            | Command::Preview
            | Command::Save
            | Command::Rename
            | Command::Delete
            | Command::Reset
    ) && presentation_locked(world)
    {
        return Err(
            "Apply or cancel the source feature or joint editor before changing its presentation"
                .into(),
        );
    }
    let state = world.get_resource::<State>().ok_or("Open Named Views")?;
    if !state.visible
        || state.owner.as_ref() != Some(&action.context)
        || state.generation != generation
    {
        return Err("The named-view controls have changed".into());
    }
    let units = engine.document_units();
    if let Command::Field(field) = command {
        if matches!(&action.control.input, ControlInput::Key(k) if k == &limo_cad_interface::KeyChord::plain("Enter"))
            && panel::choices(state, engine, *field)?.is_none()
        {
            return reduce_inner(
                world,
                handle,
                engine,
                bridge,
                action,
                generation,
                &Command::Save,
            );
        }
        let value = if let ControlInput::SetValue(value) = &action.control.input {
            value.clone()
        } else if let Some(options) = panel::choices(state, engine, *field)? {
            workbench::cam::choose(
                &options,
                &panel::field_text(state, *field),
                &action.control.input,
            )?
        } else if matches!(action.control.input, ControlInput::Key(_))
            || super::super::is_activation(&action.control.input)
        {
            return Ok(json!({"focused":true}));
        } else {
            return Err("Enter a named-view value".into());
        };
        return edit(world, engine, *field, &value, units).map(|_| json!({"handled":true}));
    }
    if !super::super::is_activation(&action.control.input) {
        return Err("Activate a named-view control".into());
    }
    if let Some(error) = saved_action_error(state, command) {
        return Err(error.into());
    }
    match command {
        Command::Close => {
            if world.resource::<State>().previewing {
                return Err("Reset or save the draft preview before closing Named Views".into());
            }
            world.resource_mut::<State>().visible = false;
            return Ok(json!({"closed":true}));
        }
        Command::Info => return Ok(json!({"read_only":true})),
        Command::PrintSettings => {
            let body = world
                .resource::<State>()
                .target
                .strip_prefix("body:")
                .and_then(|id| id.parse().ok());
            return print_intent::open(world, engine, &receipt.owner, body);
        }
        Command::Scroll(delta) => {
            let mut state = world.resource_mut::<State>();
            state.scroll = state.scroll.saturating_add_signed(*delta as isize);
            return Ok(json!({"handled":true}));
        }
        Command::Create => {
            return open(world, engine, &action.context, Some(""));
        }
        Command::Capture => {
            let name = world
                .resource::<State>()
                .draft
                .as_ref()
                .ok_or("Create a named view")?
                .name
                .clone();
            let current = capture(world, engine, name)?;
            let mut state = world.resource_mut::<State>();
            let draft = state.draft.as_mut().unwrap();
            draft.camera = current.camera;
            draft.visible_body_ids = current.visible_body_ids;
            state.report = None;
            return Ok(json!({"captured":true}));
        }
        _ => {}
    }
    let operation;
    let arguments;
    {
        let mut state = world.resource_mut::<State>();
        if matches!(
            command,
            Command::Save | Command::Preview | Command::Check | Command::ApplyCorrections
        ) {
            apply_placement(&mut state, units)?;
        }
        state.error = None;
        match command {
            Command::Check | Command::Preview => {
                let draft = state.draft.clone().ok_or("Create a named view")?;
                let checking = matches!(command, Command::Check);
                let args = if checking {
                    json!({"view":draft})
                } else {
                    serde_json::to_value(&draft).map_err(|e| e.to_string())?
                };

                let owner = receipt.owner.clone();
                let revision = receipt.revision;
                return worker::enqueue_query(
                    world,
                    receipt.owner,
                    revision,
                    if checking {
                        "print_layout_check"
                    } else {
                        "named_view_resolve"
                    }
                    .into(),
                    args,
                    move |world, services, result| {
                        services.bridge.with_native_document_receipt(
                            &services.engine,
                            &owner,
                            |current| {
                                let state = world
                                    .get_resource::<State>()
                                    .ok_or("Named view editor closed")?;
                                if current != revision
                                    || state.owner.as_ref() != Some(&owner)
                                    || state.generation != generation
                                    || state.draft.as_ref() != Some(&draft)
                                {
                                    return Err(
                                        "The named view changed; run the preview or check again"
                                            .into(),
                                    );
                                }
                                let value = match result {
                                    Ok(result) => result.value,
                                    Err(error) => {
                                        world.resource_mut::<State>().error = Some(error.clone());
                                        return Err(error);
                                    }
                                };
                                if checking {
                                    world.resource_mut::<State>().report = Some(value.clone());
                                    Ok(json!({"checked":true,"report":value}))
                                } else {
                                    apply_preview(
                                        world,
                                        &services.engine,
                                        &DocumentReceipt {
                                            owner: owner.clone(),
                                            revision: current,
                                        },
                                        &draft,
                                        serde_json::from_value(value).map_err(|e| e.to_string())?,
                                    )?;
                                    Ok(json!({"previewed":true,"saved":false}))
                                }
                            },
                        )
                    },
                );
            }
            Command::ApplyCorrections => {
                let report = state.report.clone().ok_or("Check the layout first")?;
                apply_corrections(state.draft.as_mut().ok_or("Create a named view")?, &report)?;
                state.placement = None;
                state.target.clear();
                state.report = None;
                return Ok(json!({"corrected_draft":true}));
            }
            Command::Save => {
                let draft = state.draft.as_ref().ok_or("Create a named view")?;
                if state
                    .selected
                    .as_ref()
                    .is_some_and(|name| name != &draft.name)
                {
                    return Err("Use Rename to change the saved view's name".into());
                }
                if state.selected.is_none() && state.views.iter().any(|v| v.name == draft.name) {
                    return Err("A view with that name already exists; choose it to update".into());
                }
                operation = "upsert_named_view";
                arguments = serde_json::to_value(draft).map_err(|e| e.to_string())?;
            }
            Command::Rename => {
                let name = state
                    .selected
                    .as_ref()
                    .ok_or("Select a saved view to rename")?;
                let draft = state.draft.as_ref().unwrap();
                let mut original = state.original.clone().unwrap();
                original.name = draft.name.clone();
                if state.placement_dirty || original != *draft {
                    return Err("Save other layout edits before renaming".into());
                }
                operation = "rename_named_view";
                arguments = json!({"name":name,"new_name":draft.name});
            }
            Command::Delete => {
                operation = "delete_named_view";
                arguments =
                    json!({"name":state.selected.as_ref().ok_or("Select a saved view to delete")?});
            }
            Command::Recall => {
                if state.placement_dirty || state.draft != state.original {
                    return Err("Save or reselect the named view before recalling it".into());
                }
                operation = "recall_named_view";
                arguments = json!({"name":state.selected.as_ref().ok_or("Save this view before recalling it")?});
            }
            Command::Reset => {
                operation = "clear_named_view";
                arguments = json!({});
            }
            _ => unreachable!(),
        }
    }
    let saved_name = match operation {
        "upsert_named_view" => arguments["name"].as_str().map(str::to_string),
        "rename_named_view" => arguments["new_name"].as_str().map(str::to_string),
        _ => None,
    };
    worker::enqueue_operation(
        world,
        receipt.owner,
        receipt.revision,
        operation.into(),
        arguments,
        move |world, services, result| {
            if result.is_ok() {
                if let Some(mut state) = world.get_resource_mut::<State>() {
                    if let Some(name) = saved_name {
                        state.selected = Some(name);
                    }
                    if operation == "delete_named_view" {
                        state.selected = None;
                        state.original = None;
                        state.draft = None;
                    }
                }
            }
            Ok(finish_mutation(
                &services.engine,
                &services.bridge,
                world,
                operation,
                result?,
            ))
        },
    )
}

fn edit(
    world: &mut World,
    engine: &AppState,
    field: Field,
    value: &str,
    units: UnitSystem,
) -> Result<(), String> {
    let mut state = world.resource_mut::<State>();
    state.error = None;
    match field {
        Field::Saved => {
            let view = state
                .views
                .iter()
                .find(|v| v.name == value)
                .cloned()
                .ok_or("Choose a saved named view")?;
            select(&mut state, view, true);
        }
        Field::Name => state.draft.as_mut().ok_or("Create a named view")?.name = value.into(),
        Field::Target => {
            apply_placement(&mut state, units)?;
            let options = panel::choices(&state, engine, Field::Target)?.unwrap();
            if !options.iter().any(|o| o.value == value) {
                return Err("Choose a CAD occurrence or body".into());
            }
            let pose = target_pose(state.draft.as_ref().ok_or("Create a named view")?, value);
            state.placement = Some(assembly::TransformDraft::new(pose, units));
            state.target = value.into();
        }
        Field::Translation(axis) | Field::Rotation(axis) => {
            if axis >= 3 {
                return Err("Unknown placement axis".into());
            }
            if matches!(field, Field::Rotation(_)) && state.target.starts_with("body:") {
                return Err("Choose a CAD occurrence to rotate a part".into());
            }
            let placement = state
                .placement
                .as_mut()
                .ok_or("Choose a CAD occurrence or body")?;
            if matches!(field, Field::Translation(_)) {
                placement.translation[axis].set_text(value.into());
            } else {
                placement.rotation[axis].set_text(value.into());
                placement.exact_rotation = None;
            }
            state.placement_dirty = true;
        }
        Field::PrintLayout => {
            let v = match value {
                "true" => true,
                "false" => false,
                _ => return Err("Choose presentation or print layout".into()),
            };
            state
                .draft
                .as_mut()
                .ok_or("Create a named view")?
                .print_layout = v;
        }
        Field::Printer => {
            let bed = panel::printer_choices()
                .into_iter()
                .find(|(key, _, _)| key == value)
                .map(|(_, _, bed)| bed)
                .ok_or("Choose an embedded printer bed")?;
            state.draft.as_mut().ok_or("Create a named view")?.print_bed = bed;
        }
        Field::VisibleBody => {
            let included = match value {
                "true" => true,
                "false" => false,
                _ => return Err("Choose included or excluded".into()),
            };
            let id = state
                .target
                .strip_prefix("body:")
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or("Choose a body to edit its visibility in this view")?;
            let view = state.draft.as_mut().ok_or("Create a named view")?;
            view.visible_body_ids.retain(|v| *v != id);
            if included {
                view.visible_body_ids.push(id);
            }
        }
    }
    state.report = None;
    Ok(())
}

pub(super) fn synchronize(
    world: &mut World,
    camera: Entity,
    services: &NativeServices,
    owner: &DocumentContext,
    width: f32,
    height: f32,
) -> Result<(), String> {
    let mut state = world.remove_resource::<State>().unwrap_or_default();
    state.widgets.begin();
    let result = (|| {
        if state.owner.as_ref() != Some(owner) {
            state.visible = false;
            state.draft = None;
            state.previewing = false;
            state.preview_origin = None;
            state.owner = Some(owner.clone());
            state.revision = None;
        }
        if !state.visible || print_intent::active(world) {
            return Ok(());
        }
        let receipt = services
            .bridge
            .native_document_receipt(&services.engine, owner)?;
        if state.revision != Some(receipt.revision) {
            let views = read_views(&services.engine)?;
            if let Some(name) = state.selected.clone() {
                if let Some(view) = views.views.iter().find(|v| v.name == name).cloned() {
                    if state.original.as_ref() != Some(&view) {
                        select(&mut state, view, true);
                    }
                } else {
                    state.selected = None;
                    state.original = None;
                }
            }
            state.views = views.views;
            state.revision = Some(receipt.revision);
            state.previewing = false;
            state.preview_origin = None;
            state.report = None;
        }
        panel::paint(world, camera, &mut state, &services.engine, width, height)
    })();
    state.widgets.finish(world);
    world.insert_resource(state);
    result
}
