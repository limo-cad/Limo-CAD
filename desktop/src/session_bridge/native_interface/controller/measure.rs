//! Disposable Fusion-style inspection. Ordered selections are distinct from
//! parent-body ownership, and may reference two occurrences of the same body.
use super::*;
use bevy::math::{DQuat, DVec3};
use limo_cad_core::{BodyId, FaceId, UnitSystem};
use limo_cad_interface::{ChoiceOption, Field as ControlField, KeyChord};
use limo_cad_occt::measurement::{
    MeasurementReport, MeasurementRequest, MeasurementSelection, MeasurementTarget,
};
use native_viewport::{
    NativePick, NativePickPurpose, ViewportMeasurementLine, ViewportPresentation,
};

mod overlay;
mod panel;
mod readout;
use overlay::present;
use panel::paint;
use readout::{rows, Guide};
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    Filter,
    Precision,
    Secondary,
    Delta,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Open,
    Close,
    Clear,
    SnapPoints,
    Field(Field),
    Copy(usize),
    Scroll(i32),
}

#[derive(Resource)]
struct State {
    owner: Option<DocumentContext>,
    revision: u64,
    generation: u64,
    visible: bool,
    filter: String,
    precision: String,
    units: UnitSystem,
    secondary: String,
    delta: String,
    snaps: bool,
    selections: Vec<MeasurementSelection>,
    report: Option<MeasurementReport>,
    bodies: Arc<Vec<limo_cad_sketch::BodyPoseDto>>,
    instances: Arc<Vec<limo_cad_sketch::InstanceBodyPoseDto>>,
    hover_points: Vec<[f32; 3]>,
    message: String,
    scroll: usize,
    widgets: chrome::Widgets,
    original: Option<ViewportPresentation>,
    overlay_key: Option<OverlayKey>,
    overlay_lines: Arc<Vec<ViewportMeasurementLine>>,
    overlay_points: Arc<Vec<[f32; 3]>>,
}
#[derive(PartialEq)]
struct OverlayKey {
    generation: u64,
    units: UnitSystem,
    hovered: Option<Guide>,
    hover_points: Vec<[f32; 3]>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            owner: None,
            revision: 0,
            generation: 0,
            visible: false,
            filter: "geometry".into(),
            precision: "3".into(),
            units: UnitSystem::Mm,
            secondary: "none".into(),
            delta: "global".into(),
            snaps: true,
            selections: vec![],
            report: None,
            bodies: Arc::default(),
            instances: Arc::default(),
            hover_points: vec![],
            message: String::new(),
            scroll: 0,
            widgets: default(),
            original: None,
            overlay_key: None,
            overlay_lines: Arc::default(),
            overlay_points: Arc::default(),
        }
    }
}
impl State {
    fn invalidate(&mut self) {
        self.generation = self.generation.saturating_add(1);
        self.report = None;
        self.hover_points.clear();
        self.scroll = 0;
        self.message.clear();
    }
    fn clear(&mut self) {
        self.selections.clear();
        self.invalidate();
    }
    fn select(&mut self, selection: MeasurementSelection) {
        if let Some(index) = self.selections.iter().position(|s| *s == selection) {
            self.selections.remove(index);
        } else {
            if self.selections.len() == 2 {
                self.selections.clear();
            }
            self.selections.push(selection);
        }
        self.invalidate();
    }
    fn value(&self, field: Field) -> &str {
        match field {
            Field::Filter => &self.filter,
            Field::Precision => &self.precision,
            Field::Secondary => &self.secondary,
            Field::Delta => &self.delta,
        }
    }
    fn options(field: Field) -> Vec<ChoiceOption> {
        let items: Vec<(String, String)> = match field {
            Field::Filter => [
                ("geometry", "Faces / Edges / Vertices"),
                ("body", "Bodies"),
                ("vertex", "Vertices"),
            ]
            .map(|(v, l)| (v.into(), l.into()))
            .into(),
            Field::Precision => (0..=6)
                .map(|i| (i.to_string(), format!("{i} decimal places")))
                .collect(),
            Field::Secondary => [
                ("none", "None"),
                ("mm", "Millimetres"),
                ("cm", "Centimetres"),
                ("in", "Inches"),
            ]
            .map(|(v, l)| (v.into(), l.into()))
            .into(),
            Field::Delta => [
                ("none", "None"),
                ("global", "Global"),
                ("first", "Selection 1"),
                ("second", "Selection 2"),
            ]
            .map(|(v, l)| (v.into(), l.into()))
            .into(),
        };
        items
            .into_iter()
            .map(|(value, label)| ChoiceOption {
                value,
                label,
                disabled: false,
            })
            .collect()
    }
    fn rotation(&self, index: usize) -> DQuat {
        let Some(s) = self.selections.get(index) else {
            return DQuat::IDENTITY;
        };
        let rotation = s
            .occurrence_id
            .and_then(|id| {
                self.instances
                    .iter()
                    .find(|p| p.occurrence_id.0 == id && p.body_id == s.body_id)
                    .map(|p| p.rotation)
            })
            .or_else(|| {
                self.bodies
                    .iter()
                    .find(|p| p.body_id == s.body_id)
                    .map(|p| p.rotation)
            })
            .unwrap_or([0., 0., 0., 1.]);
        DQuat::from_array(rotation).normalize()
    }
    fn delta_rotation(&self) -> DQuat {
        match self.delta.as_str() {
            "first" => self.rotation(0),
            "second" => self.rotation(1),
            _ => DQuat::IDENTITY,
        }
    }
}

pub(crate) fn active(world: &World) -> bool {
    world.get_resource::<State>().is_some_and(|s| s.visible)
}
pub(crate) fn available(world: &World) -> bool {
    workbench::workspace(world) == workbench::Workspace::Solid
        && feature::panel(world).is_none()
        && !assembly::joint::active(world)
        && !named_views::active(world)
        && !print_intent::active(world)
        && !section_review::active(world)
        && native_viewport::interface_geometry(world)
            .active_sketch
            .is_none()
}
fn initial(
    view: &ViewportPresentation,
    scene: &limo_cad_solid::SolidSceneDto,
) -> Vec<MeasurementSelection> {
    let mut selections = vec![];
    let occurrence = |body_id| {
        view.selected_occurrence_id.or_else(|| {
            let mut poses = view
                .instance_body_poses
                .iter()
                .filter(|p| p.body_id == body_id && p.visible);
            let id = poses.next()?.occurrence_id.0;
            poses.next().is_none().then_some(id)
        })
    };
    for body in &scene.bodies {
        for edge in &body.edges {
            if view.selected_edge_ids.contains(&edge.id.0) {
                selections.push(MeasurementSelection {
                    body_id: body.id,
                    occurrence_id: occurrence(body.id),
                    target: MeasurementTarget::Edge { edge_id: edge.id },
                });
            }
        }
        for face in &body.faces {
            if view.selected_face_ids.contains(&face.id.0) {
                selections.push(MeasurementSelection {
                    body_id: body.id,
                    occurrence_id: occurrence(body.id),
                    target: MeasurementTarget::Face { face_id: face.id },
                });
            }
        }
    }
    if selections.is_empty() {
        selections = view
            .selected_body_ids
            .iter()
            .map(|id| MeasurementSelection {
                body_id: BodyId(*id),
                occurrence_id: occurrence(BodyId(*id)),
                target: MeasurementTarget::Body,
            })
            .collect();
    }
    selections.truncate(2);
    selections
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
    handle.validate_action(action)?;
    bridge
        .with_native_document_receipt(engine, &action.context, |revision| {
            if *command == Command::Open {
                if !super::super::is_activation(&action.control.input) || !available(world) {
                    return Err("Finish the current tool before opening Measure".into());
                }
                let mut state = world.remove_resource::<State>().unwrap_or_default();
                state.clear();
                state.owner = Some(action.context.clone());
                state.revision = revision;
                state.visible = true;
                state.units = engine.document_units();
                let (_, _, view, _) = native_viewport::interface_view(world);
                state.selections = initial(view, native_viewport::interface_geometry(world).scene);
                state.bodies = view.body_poses.clone();
                state.instances = view.instance_body_poses.clone();
                state.original = Some(view.clone());
                let mut cleared = view.clone();
                super::super::clear_selection(&mut cleared);
                native_viewport::apply_interface_view(
                    world,
                    &action.context.document_id,
                    None,
                    Some(cleared),
                )?;
                world.insert_resource(state);
                workbench::execute(world, &workbench::Command::Dismiss)?;
                // Queue after the publisher fence is released below.
                return Ok(json!({"opened":true}));
            }
            let state = world.get_resource::<State>().ok_or("Open Measure")?;
            if !state.visible
                || state.owner.as_ref() != Some(&action.context)
                || state.revision != revision
                || state.generation != generation
            {
                return Err("The measurement controls or document changed".into());
            }
            if let Command::Field(field) = command {
                let options = State::options(*field);
                let value =
                    workbench::cam::choose(&options, state.value(*field), &action.control.input)?;
                let mut state = world.resource_mut::<State>();
                match field {
                    Field::Filter => {
                        state.filter = value;
                        state.clear();
                    }
                    Field::Precision => {
                        state.precision = value;
                        state.generation += 1;
                    }
                    Field::Secondary => {
                        state.secondary = value;
                        state.generation += 1;
                    }
                    Field::Delta => {
                        state.delta = value;
                        state.generation += 1;
                    }
                }
                return Ok(json!({"changed":true}));
            }
            if !super::super::is_activation(&action.control.input) {
                return Err("Activate a Measure control".into());
            }
            if let Command::Copy(index) = command {
                let rows = rows(state, engine.document_units());
                let row = rows
                    .get(*index)
                    .filter(|r| !r.value.is_empty())
                    .ok_or("The measurement result changed")?;
                let value = row.value.clone();
                world
                    .get_resource_mut::<bevy::clipboard::Clipboard>()
                    .ok_or("Native clipboard unavailable")?
                    .set_text(value)
                    .map_err(|e| format!("Cannot copy measurement: {e:?}"))?;
                world.resource_mut::<State>().message = "Measurement copied".into();
                return Ok(json!({"copied":true}));
            }
            let mut state = world.resource_mut::<State>();
            match command {
                Command::Close => {
                    state.visible = false;
                    state.clear();
                }
                Command::Clear => state.clear(),
                Command::SnapPoints => {
                    state.snaps = !state.snaps;
                    state.hover_points.clear();
                }
                Command::Scroll(amount) => {
                    state.scroll = state.scroll.saturating_add_signed(*amount as isize)
                }
                _ => return Err("Unsupported Measure control".into()),
            }
            Ok(json!({"handled":true}))
        })
        .and_then(|value| {
            if *command == Command::Open && !world.resource::<State>().selections.is_empty() {
                queue(world, engine, bridge, &action.context)
            } else {
                if matches!(
                    command,
                    Command::Close | Command::Clear | Command::Field(Field::Filter)
                ) {
                    if *command == Command::Close {
                        restore_selection(world, &action.context)?;
                    }
                    present(world, &action.context)?;
                }
                Ok(value)
            }
        })
}

fn queue(
    world: &mut World,
    engine: &AppState,
    bridge: &SessionBridgeState,
    owner: &DocumentContext,
) -> Result<Value, String> {
    let revision = bridge.native_document_receipt(engine, owner)?.revision;
    let state = world.resource::<State>();
    let request = MeasurementRequest {
        selections: state.selections.clone(),
    };
    let generation = state.generation;
    let expected = owner.clone();
    if request.selections.is_empty() {
        return Ok(json!({"cleared":true}));
    }
    world.resource_mut::<State>().message = "Measuring…".into();
    worker::enqueue_measurement_review(
        world,
        owner.clone(),
        revision,
        request,
        move |world, services, result| {
            let state = world.get_resource::<State>().ok_or("Measure was closed")?;
            if !state.visible
                || state.owner.as_ref() != Some(&expected)
                || state.generation != generation
                || state.revision != revision
            {
                return Ok(json!({"retired":true}));
            }
            let result = result.and_then(|finished| {
                services.bridge.with_native_document_receipt(
                    &services.engine,
                    &expected,
                    |current| {
                        if current != revision {
                            return Err("The measurement source changed".into());
                        }
                        services.engine.validate_measurement_review(&finished)
                    },
                )?;
                Ok(finished.into_report())
            });
            let mut state = world.resource_mut::<State>();
            match result {
                Ok(report) => {
                    state.report = Some(report);
                    state.message.clear();
                }
                Err(error) => {
                    state.report = None;
                    state.message = error;
                }
            }
            state.generation = state.generation.saturating_add(1);
            present(world, &expected)?;
            Ok(json!({"measured":world.resource::<State>().report.is_some()}))
        },
    )
}

fn pick(
    world: &World,
    owner: &DocumentContext,
    cursor: [f32; 2],
    lock: bool,
    hide: bool,
) -> Result<Option<(NativePick, bool)>, String> {
    let state = world.resource::<State>();
    let get = |purpose| native_viewport::interface_pick(world, &owner.document_id, cursor, purpose);
    if state.filter == "vertex" || state.filter == "geometry" && (state.snaps && !hide || lock) {
        if let Some(hit) = get(NativePickPurpose::Vertex)? {
            return Ok(Some((hit, true)));
        }
        if state.filter == "geometry" {
            if let Some(hit) = get(NativePickPurpose::CurveCenter)? {
                return Ok(Some((hit, true)));
            }
        }
        if state.filter == "vertex" || lock {
            return Ok(None);
        }
    }
    if state.filter == "geometry" {
        if let Some(hit) = get(NativePickPurpose::Edge)? {
            return Ok(Some((hit, false)));
        }
    }
    Ok(get(NativePickPurpose::Geometry)?.map(|hit| (hit, false)))
}

pub(crate) fn canvas(
    world: &mut World,
    services: &NativeServices,
    owner: &DocumentContext,
    cursor: Option<[f32; 2]>,
    clicked: bool,
    lock: bool,
    hide: bool,
) -> Result<Option<Value>, String> {
    if !active(world) || !available(world) {
        return Ok(None);
    }
    let receipt = services
        .bridge
        .native_document_receipt(&services.engine, owner)?;
    let state = world.resource::<State>();
    if state.owner.as_ref() != Some(owner) || state.revision != receipt.revision {
        let mut state = world.resource_mut::<State>();
        state.visible = false;
        state.clear();
        state.original = None;
        present(world, owner)?;
        return Ok(Some(json!({"handled":true,"retired":true})));
    }
    let hit = cursor
        .map(|p| pick(world, owner, p, lock, hide))
        .transpose()?
        .flatten();
    let mut next = None;
    let mut hover_points = vec![];
    if let Some((hit, vertex)) = &hit {
        let transform =
            native_viewport::interface_body_transform(world, hit.body_id, hit.occurrence_id);
        let body = native_viewport::interface_geometry(world)
            .scene
            .bodies
            .iter()
            .find(|b| b.id.0 == hit.body_id)
            .ok_or("Measurement body disappeared")?;
        let edge = hit
            .edge_id
            .and_then(|id| body.edges.iter().find(|e| e.id.0 == id));
        let target = if *vertex {
            let edge = edge.ok_or("Measurement vertex has no edge")?;
            if hit.connector_kind.as_deref() == Some("measurement_center") {
                MeasurementTarget::Center { edge_id: edge.id }
            } else {
                let at = |p: &limo_cad_solid::Point3Dto| {
                    transform.transform_point(Vec3::new(p.x as f32, p.y as f32, p.z as f32))
                };
                let first = at(edge
                    .points
                    .first()
                    .ok_or("Measurement vertex disappeared")?);
                let last = at(edge.points.last().ok_or("Measurement vertex disappeared")?);
                MeasurementTarget::Vertex {
                    edge_id: edge.id,
                    end: last.distance_squared(Vec3::from_array(hit.point))
                        < first.distance_squared(Vec3::from_array(hit.point)),
                }
            }
        } else if world.resource::<State>().filter == "body" {
            MeasurementTarget::Body
        } else if let Some(edge) = edge {
            MeasurementTarget::Edge { edge_id: edge.id }
        } else {
            MeasurementTarget::Face {
                face_id: FaceId(hit.face_id),
            }
        };
        if world.resource::<State>().snaps
            && !hide
            && world.resource::<State>().filter == "geometry"
            || *vertex
        {
            for edge in body.edges.iter().filter(|e| {
                hit.edge_id == Some(e.id.0)
                    || hit.edge_id.is_none()
                        && body
                            .faces
                            .iter()
                            .any(|f| f.id.0 == hit.face_id && f.edge_keys.contains(&e.key))
            }) {
                for p in edge.points.first().into_iter().chain(edge.points.last()) {
                    let p = transform
                        .transform_point(Vec3::new(p.x as f32, p.y as f32, p.z as f32))
                        .to_array();
                    if !hover_points.contains(&p) {
                        hover_points.push(p);
                    }
                }
                if let Some(circle) = edge.circle {
                    let p = circle.center;
                    hover_points.push(
                        transform
                            .transform_point(Vec3::new(p.x as f32, p.y as f32, p.z as f32))
                            .to_array(),
                    );
                }
            }
        }
        next = Some(MeasurementSelection {
            body_id: BodyId(hit.body_id),
            occurrence_id: hit.occurrence_id,
            target,
        });
    }
    let (_, _, mut view, _) = native_viewport::interface_view_snapshot(world);
    view.hovered_body_id = hit.as_ref().map(|(h, _)| h.body_id);
    view.hovered_occurrence_id = hit.as_ref().and_then(|(h, _)| h.occurrence_id);
    view.hovered_edge_id = hit.as_ref().and_then(|(h, _)| h.edge_id);
    view.hovered_face_id = hit
        .as_ref()
        .filter(|(h, _)| h.edge_id.is_none())
        .map(|(h, _)| h.face_id);
    native_viewport::apply_interface_view(world, &owner.document_id, None, Some(view))?;
    if clicked {
        if let Some(selection) = next {
            world.resource_mut::<State>().select(selection);
            present(world, owner)?;
            return queue(world, &services.engine, &services.bridge, owner).map(Some);
        }
    }
    world.resource_mut::<State>().hover_points = hover_points;
    present(world, owner)?;
    Ok(Some(json!({"handled":true})))
}

pub(crate) fn escape(world: &mut World, owner: &DocumentContext) -> Result<bool, String> {
    if !active(world) {
        return Ok(false);
    }
    let mut state = world.resource_mut::<State>();
    state.visible = false;
    state.clear();
    restore_selection(world, owner)?;
    present(world, owner)?;
    Ok(true)
}

pub(crate) fn clear_current(world: &mut World, owner: &DocumentContext) -> Result<Value, String> {
    let mut state = world.resource_mut::<State>();
    if state.owner.as_ref() != Some(owner) {
        return Err("The measurement document changed".into());
    }
    state.clear();
    present(world, owner)?;
    Ok(json!({"cleared":true}))
}

fn restore_selection(world: &mut World, owner: &DocumentContext) -> Result<(), String> {
    let state = world.resource::<State>();
    let (_, _, view, _) = native_viewport::interface_view(world);
    let current = state.owner.as_ref() == Some(owner)
        && state.bodies == view.body_poses
        && state.instances == view.instance_body_poses
        && world
            .get_resource::<NativeServices>()
            .is_none_or(|services| {
                services
                    .bridge
                    .native_document_receipt(&services.engine, owner)
                    .is_ok_and(|receipt| receipt.revision == state.revision)
            });
    let original = world.resource_mut::<State>().original.take();
    if let Some(original) = original.filter(|_| current) {
        let (_, _, mut view, _) = native_viewport::interface_view_snapshot(world);
        view.selected_body_ids = original.selected_body_ids;
        view.selected_face_ids = original.selected_face_ids;
        view.selected_edge_ids = original.selected_edge_ids;
        view.selected_occurrence_id = original.selected_occurrence_id;
        view.selected_surface_point = original.selected_surface_point;
        native_viewport::apply_interface_view(world, &owner.document_id, None, Some(view))?;
    }
    Ok(())
}

pub(crate) fn synchronize(
    world: &mut World,
    camera: Entity,
    services: &NativeServices,
    owner: &DocumentContext,
    width: f32,
    height: f32,
) -> Result<(), String> {
    let allowed = available(world);
    let mut state = world.remove_resource::<State>().unwrap_or_default();
    state.widgets.begin();
    let result = (|| {
        if state.owner.as_ref() != Some(owner) || state.visible && !allowed {
            state.visible = false;
            state.clear();
            state.original = None;
            state.owner = Some(owner.clone());
        }
        if !state.visible {
            return Ok(());
        }
        let revision = services
            .bridge
            .native_document_receipt(&services.engine, owner)?
            .revision;
        let (_, _, view, _) = native_viewport::interface_view(world);
        if state.revision != revision
            || state.bodies != view.body_poses
            || state.instances != view.instance_body_poses
        {
            state.clear();
            state.original = None;
            state.revision = revision;
            state.bodies = view.body_poses.clone();
            state.instances = view.instance_body_poses.clone();
            state.message = "Model or placement changed. Select objects again.".into();
        }
        paint(
            world,
            camera,
            &mut state,
            services.engine.document_units(),
            width,
            height,
        )
    })();
    state.widgets.finish(world);
    world.insert_resource(state);
    present(world, owner)?;
    result
}
