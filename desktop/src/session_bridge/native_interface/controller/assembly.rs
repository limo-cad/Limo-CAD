//! The assembly browser owns selection and small typed edit drafts. Committed
//! operations use the shared dispatcher and the native document worker.
use super::*;
use crate::native_forms::{DimensionKind, MeasurementInput};
use crate::session_bridge::parse_engine_envelope;
use bevy::math::{DQuat, EulerRot};
use limo_cad_core::UnitSystem;
use limo_cad_interface::{ControlInput, Field};
use limo_cad_sketch::{
    AssemblyDocumentDto, AssemblyTransformDto, ComponentDefinitionDto, ComponentOccurrenceDto,
};
use std::collections::HashSet;
mod inspect;
pub(crate) mod joint;
pub(crate) mod motion;
mod panel;
pub(crate) mod studies;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tab {
    #[default]
    Structure,
    Inspect,
    Motion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditField {
    Name,
    DefinitionName,
    Translation(bool, usize),
    Rotation(bool, usize),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Motion(motion::Action),
    Study(studies::Action),
    Tab(Tab),
    Inspect(inspect::Action),
    Joint(joint::Command),
    Show(bool),
    Select(u64),
    Expand(u64),
    Components,
    Inspector,
    Visibility(u64),
    Ground(u64),
    Duplicate(u64),
    Remove(u64),
    Move(u64),
    Create(bool),
    Definitions,
    Definition(u64),
    Add(bool),
    Edit(u64, EditField),
    Rename(u64, bool),
    ApplyTransform(u64, bool),
    Parents,
    Parent(u64, Option<u64>),
    Scroll(i32),
}

#[derive(Clone)]
pub(super) struct TransformDraft {
    pub(super) translation: [MeasurementInput; 3],
    pub(super) rotation: [MeasurementInput; 3],
    pub(super) exact_rotation: Option<[f64; 4]>,
}
impl TransformDraft {
    pub(super) fn new(value: AssemblyTransformDto, units: UnitSystem) -> Self {
        let (z, y, x) = DQuat::from_array(value.rotation)
            .normalize()
            .to_euler(EulerRot::ZYX);
        Self {
            translation: value
                .translation
                .map(|v| MeasurementInput::new(DimensionKind::Length, v, units)),
            rotation: [x, y, z]
                .map(|v| MeasurementInput::new(DimensionKind::Angle, v.to_degrees(), units)),
            exact_rotation: Some(value.rotation),
        }
    }
    pub(super) fn value(&self, units: UnitSystem) -> Result<AssemblyTransformDto, String> {
        let mut t = [0.; 3];
        let mut r = [0.; 3];
        for i in 0..3 {
            t[i] = self.translation[i].evaluate(units, &[])?;
            r[i] = self.rotation[i].evaluate(units, &[])?.to_radians();
        }
        if t.iter().any(|v| v.abs() > f32::MAX as f64 / 16.) {
            return Err("Placement exceeds the display range".into());
        }
        Ok(AssemblyTransformDto {
            translation: t,
            rotation: self.exact_rotation.unwrap_or_else(|| {
                DQuat::from_euler(EulerRot::ZYX, r[2], r[1], r[0])
                    .normalize()
                    .to_array()
            }),
        })
    }
}
struct Draft {
    occurrence: u64,
    name: String,
    definition_name: String,
    placement: TransformDraft,
    origin: TransformDraft,
}
impl Draft {
    fn new(o: &ComponentOccurrenceDto, d: &ComponentDefinitionDto, units: UnitSystem) -> Self {
        Self {
            occurrence: o.id.0,
            name: o.name.clone(),
            definition_name: d.name.clone(),
            placement: TransformDraft::new(o.local_pose, units),
            origin: TransformDraft::new(d.local_coordinate_system, units),
        }
    }
    fn set(&mut self, field: EditField, text: &str) -> Result<(), String> {
        match field {
            EditField::Name => self.name = text.into(),
            EditField::DefinitionName => self.definition_name = text.into(),
            EditField::Translation(origin, i) | EditField::Rotation(origin, i) => {
                let f = if origin {
                    &mut self.origin
                } else {
                    &mut self.placement
                };
                if i >= 3 {
                    return Err("Unknown placement axis".into());
                }
                if matches!(field, EditField::Translation(..)) {
                    f.translation[i].set_text(text.into());
                } else {
                    f.rotation[i].set_text(text.into());
                    f.exact_rotation = None;
                }
            }
        }
        Ok(())
    }
}
#[derive(Resource)]
struct Browser {
    motion: motion::State,
    studies: studies::State,
    tab: Tab,
    inspect: inspect::State,
    enabled: bool,
    owner: Option<DocumentContext>,
    revision: u64,
    assembly: Option<Arc<AssemblyDocumentDto>>,
    units: UnitSystem,
    collapsed: HashSet<u64>,
    components: bool,
    inspector: bool,
    definition: Option<u64>,
    definitions_open: bool,
    parents_open: bool,
    draft: Option<Draft>,
    scroll: f32,
    max_scroll: f32,
    widgets: chrome::Widgets,
}
impl Default for Browser {
    fn default() -> Self {
        Self {
            motion: default(),
            studies: default(),
            tab: Tab::Structure,
            inspect: default(),
            enabled: false,
            owner: None,
            revision: 0,
            assembly: None,
            units: UnitSystem::Mm,
            collapsed: HashSet::new(),
            components: true,
            inspector: true,
            definition: None,
            definitions_open: false,
            parents_open: false,
            draft: None,
            scroll: 0.,
            max_scroll: 0.,
            widgets: default(),
        }
    }
}
pub(crate) fn active(world: &World) -> bool {
    world.get_resource::<Browser>().is_some_and(|s| s.enabled)
}
fn document(engine: &AppState) -> Result<AssemblyDocumentDto, String> {
    serde_json::from_value(parse_engine_envelope(
        engine.engine_call("assembly_document", ""),
    )?)
    .map_err(|e| e.to_string())
}
fn find(a: &AssemblyDocumentDto, id: u64) -> Result<&ComponentOccurrenceDto, String> {
    a.component_structure
        .occurrences
        .iter()
        .find(|o| o.id.0 == id)
        .ok_or_else(|| "The component instance no longer exists".into())
}
fn descendants(a: &AssemblyDocumentDto, id: u64) -> HashSet<u64> {
    let mut ids = HashSet::from([id]);
    loop {
        let before = ids.len();
        for o in &a.component_structure.occurrences {
            if o.parent_occurrence_id.is_some_and(|p| ids.contains(&p.0)) {
                ids.insert(o.id.0);
            }
        }
        if ids.len() == before {
            return ids;
        }
    }
}
fn select(
    world: &mut World,
    owner: &DocumentContext,
    a: &AssemblyDocumentDto,
    id: u64,
) -> Result<(), String> {
    let o = find(a, id)?;
    let (_, _, mut view, _) = native_viewport::interface_view_snapshot(world);
    view.selected_occurrence_id = Some(id);
    view.selected_body_ids = a
        .component_structure
        .definitions
        .iter()
        .find(|d| d.id == o.component_id)
        .map(|d| d.body_ids.iter().map(|b| b.0).collect())
        .unwrap_or_default();
    view.selected_face_ids.clear();
    view.selected_edge_ids.clear();
    view.selected_profiles.clear();
    view.selected_surface_point = None;
    native_viewport::apply_interface_view(world, &owner.document_id, None, Some(view))
}
fn mutation(
    world: &mut World,
    engine: &AppState,
    bridge: &SessionBridgeState,
    owner: &DocumentContext,
    revision: u64,
    operation: &str,
    args: Value,
) -> Result<Value, String> {
    let owner = owner.clone();
    let operation = operation.to_owned();
    let after_op = operation.clone();
    if worker::available(world) {
        return worker::enqueue_transaction(
            world,
            operation.clone(),
            move |services, guard| {
                services.bridge.apply_native_mutation_at(
                    &services.engine,
                    &owner,
                    revision,
                    &operation,
                    &args,
                    || guard.validate(),
                )
            },
            move |world, services, result| {
                Ok(finish_mutation(
                    &services.engine,
                    &services.bridge,
                    world,
                    &after_op,
                    result?,
                ))
            },
        );
    }
    let result =
        bridge.apply_native_mutation_at(engine, &owner, revision, &operation, &args, || Ok(()))?;
    Ok(finish_mutation(engine, bridge, world, &operation, result))
}
pub(crate) fn reduce(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    engine: &AppState,
    bridge: &SessionBridgeState,
    action: &NativeInterfaceAction,
    command: &Command,
) -> Result<Value, String> {
    let joint_command = match command {
        Command::Joint(command) => Some(command.clone()),
        Command::Motion(motion::Action::Select(id))
            if matches!(action.control.input, ControlInput::DoubleClick) =>
        {
            Some(joint::Command::Open(Some(*id)))
        }
        _ => None,
    };
    if let Some(command) = joint_command {
        bridge.with_native_document_receipt(engine, &action.context, |revision| {
            handle.validate_action(action)?;
            motion::cancel(world, &action.context, revision)?;
            studies::cancel(world, &action.context, revision)
        })?;
        world.init_resource::<Browser>();
        world.resource_mut::<Browser>().enabled = true;
        return joint::reduce(world, handle, engine, bridge, action, &command);
    }
    let receipt = bridge.native_document_receipt(engine, &action.context)?;
    bridge
        .with_native_document_owner(engine, &action.context, || handle.validate_action(action))?;
    if let Command::Show(enabled) = command {
        if !super::super::is_activation(&action.control.input) {
            return Err("Activate the assembly browser".into());
        }
        if *enabled
            && native_viewport::interface_geometry(world)
                .active_sketch
                .is_some()
        {
            return Err("Finish the active sketch before opening the assembly browser".into());
        }
        if !enabled {
            motion::cancel(world, &action.context, receipt.revision)?;
            studies::cancel(world, &action.context, receipt.revision)?;
        }
        world.init_resource::<Browser>();
        world.resource_mut::<Browser>().enabled = *enabled;
        return Ok(json!({"assembly_browser":enabled}));
    }
    let mut state = world
        .remove_resource::<Browser>()
        .ok_or("Open the assembly browser")?;
    let result = (|| {
        if !state.enabled
            || state.owner.as_ref() != Some(&receipt.owner)
            || state.revision != receipt.revision
        {
            return Err("The assembly changed; use its refreshed controls".into());
        }
        let a = state
            .assembly
            .clone()
            .ok_or("Assembly structure is unavailable")?;
        let input = &action.control.input;
        if let Command::Study(command) = command {
            if feature::panel(world).is_some()
                || joint::active(world)
                || native_viewport::interface_geometry(world)
                    .active_sketch
                    .is_some()
            {
                return Err(
                    "Finish the active modeling command before editing motion studies".into(),
                );
            }
            return studies::reduce(
                world,
                handle,
                (engine, bridge, &receipt.owner),
                receipt.revision,
                &mut state.studies,
                &a,
                (command, input),
            );
        }
        if let Command::Motion(command) = command {
            if feature::panel(world).is_some()
                || joint::active(world)
                || native_viewport::interface_geometry(world)
                    .active_sketch
                    .is_some()
            {
                return Err("Finish the active modeling command before moving joints".into());
            }
            return motion::reduce(
                world,
                (engine, bridge, &receipt.owner),
                receipt.revision,
                &mut state.motion,
                &a,
                command,
                input,
            );
        }
        if let Command::Inspect(command) = command {
            if feature::panel(world).is_some()
                || joint::active(world)
                || native_viewport::interface_geometry(world)
                    .active_sketch
                    .is_some()
            {
                return Err(
                    "Finish the active modeling command before inspecting the assembly".into(),
                );
            }
            return inspect::reduce(
                world,
                (engine, bridge, &receipt.owner),
                receipt.revision,
                &mut state.inspect,
                &a,
                state.units,
                (command, input),
            );
        }
        let selected = native_viewport::interface_view(world)
            .2
            .selected_occurrence_id;
        let normalized = match (command, input) {
            (Command::Select(id), ControlInput::Key(key))
                if key.key == "Delete" && !key.ctrl && !key.meta && !key.alt && !key.shift =>
            {
                if selected != Some(*id) || handle.focused_key() != Some(action.control.key) {
                    return Err(
                        "Focus the selected component row before removing its instance".into(),
                    );
                }
                Command::Remove(*id)
            }
            (Command::Definitions | Command::Parents, ControlInput::Key(key))
                if !super::super::is_activation(input) =>
            {
                if key.ctrl || key.meta || key.alt || key.shift {
                    return Err("This field key is not supported".into());
                }
                let (options, selected) = if matches!(command, Command::Definitions) {
                    (
                        a.component_structure
                            .definitions
                            .iter()
                            .map(|d| Some(d.id.0))
                            .collect::<Vec<_>>(),
                        state.definition,
                    )
                } else {
                    let id = state.draft.as_ref().ok_or("Select a component")?.occurrence;
                    let excluded = descendants(&a, id);
                    (
                        std::iter::once(None)
                            .chain(
                                a.component_structure
                                    .occurrences
                                    .iter()
                                    .filter(|o| !excluded.contains(&o.id.0))
                                    .map(|o| Some(o.id.0)),
                            )
                            .collect(),
                        find(&a, id)?.parent_occurrence_id.map(|p| p.0),
                    )
                };
                let index = options.iter().position(|id| *id == selected).unwrap_or(0);
                let next = match key.key.as_str() {
                    "ArrowDown" => index.saturating_add(1).min(options.len().saturating_sub(1)),
                    "ArrowUp" => index.saturating_sub(1),
                    "Home" => 0,
                    "End" => options.len().saturating_sub(1),
                    _ => return Err("This field key is not supported".into()),
                };
                let value = *options.get(next).ok_or("This field has no choices")?;
                if matches!(command, Command::Definitions) {
                    Command::Definition(value.ok_or("Choose a component")?)
                } else {
                    Command::Parent(state.draft.as_ref().unwrap().occurrence, value)
                }
            }
            (Command::Definitions, ControlInput::SetValue(value)) => Command::Definition(
                value
                    .parse()
                    .map_err(|_| "Choose an available component definition")?,
            ),
            (Command::Parents, ControlInput::SetValue(value)) => Command::Parent(
                state.draft.as_ref().ok_or("Select a component")?.occurrence,
                if value == "root" {
                    None
                } else {
                    Some(value.parse().map_err(|_| "Choose an available parent")?)
                },
            ),
            (Command::Edit(id, EditField::Name), ControlInput::Key(key)) if key.key == "Enter" => {
                Command::Rename(*id, false)
            }
            (Command::Edit(id, EditField::DefinitionName), ControlInput::Key(key))
                if key.key == "Enter" =>
            {
                Command::Rename(*id, true)
            }
            _ => command.clone(),
        };
        let choice_input = normalized != *command;
        let command = &normalized;
        if let Command::Edit(id, field) = command {
            let draft = state
                .draft
                .as_mut()
                .filter(|d| d.occurrence == *id)
                .ok_or("The component editor changed")?;
            return match input {
                ControlInput::SetValue(value) => {
                    draft.set(*field, value)?;
                    Ok(json!({"changed":true}))
                }
                input if super::super::is_activation(input) => Ok(json!({"focused":true})),
                _ => Err("Edit the component field with text".into()),
            };
        }
        if !choice_input && !super::super::is_activation(input) {
            return Err("Activate an assembly control".into());
        }
        let mut request = None;
        match *command {
            Command::Tab(tab) => {
                if tab != state.tab {
                    studies::restore(world, &mut state.studies, &receipt.owner)?;
                    motion::restore(world, &mut state.motion, &receipt.owner)?;
                    state.motion.changed(&a);
                }
                state.tab = tab;
                state.scroll = 0.;
            }
            Command::Select(id) => {
                if let Some(p) = feature::panel(world)
                    .filter(|p| p.pick_target == Some(feature::SolidField::Bodies))
                {
                    return feature::accept_pick(
                        engine,
                        bridge,
                        world,
                        &receipt.owner,
                        p.form_id,
                        feature::FeaturePick::Occurrence(id),
                        || handle.validate_action(action),
                    );
                }
                motion::restore(world, &mut state.motion, &receipt.owner)?;
                studies::restore(world, &mut state.studies, &receipt.owner)?;
                state.motion = default();
                select(world, &receipt.owner, &a, id)?;
            }
            Command::Expand(id) => {
                find(&a, id)?;
                if !state.collapsed.remove(&id) {
                    state.collapsed.insert(id);
                }
            }
            Command::Components => state.components = !state.components,
            Command::Inspector => state.inspector = !state.inspector,
            Command::Scroll(delta) => {
                state.scroll = (state.scroll + delta as f32).clamp(0., state.max_scroll)
            }
            Command::Definitions => state.definitions_open = !state.definitions_open,
            Command::Definition(id) => {
                if !a
                    .component_structure
                    .definitions
                    .iter()
                    .any(|d| d.id.0 == id)
                {
                    return Err("The reusable definition no longer exists".into());
                }
                state.definition = Some(id);
                state.definitions_open = false;
            }
            Command::Parents => state.parents_open = !state.parents_open,
            Command::Visibility(id) => {
                let o = find(&a, id)?;
                request = Some((
                    "assembly_update_occurrence",
                    json!({"occurrence":{"id":id,"visible":!o.visible}}),
                ));
            }
            Command::Ground(id) => {
                let o = find(&a, id)?;
                request = Some((
                    "assembly_set_occurrence_grounded",
                    json!({"occurrence_id":id,"grounded":!o.grounded}),
                ));
            }
            Command::Duplicate(id) => {
                let o = find(&a, id)?;
                request = Some((
                    "assembly_duplicate_occurrence",
                    json!({"occurrence_id":id,"parent_occurrence_id":o.parent_occurrence_id}),
                ));
            }
            Command::Remove(id) => {
                find(&a, id)?;
                if selected != Some(id) {
                    return Err("Select this component instance before removing it".into());
                }
                request = Some(("assembly_remove_occurrence", json!({"occurrence_id":id})));
            }
            Command::Move(id) => {
                select(world, &receipt.owner, &a, id)?;
                return feature::reduce(
                    engine,
                    bridge,
                    world,
                    &receipt.owner,
                    &feature::FeatureCommand::Open {
                        kind: feature::SolidFormKind::MoveCopy,
                        feature_id: None,
                    },
                    &ControlInput::Click,
                    || handle.validate_action(action),
                );
            }
            Command::Create(group) => {
                let bodies = if group {
                    vec![]
                } else {
                    native_viewport::interface_view(world)
                        .2
                        .selected_body_ids
                        .clone()
                };
                if !group && bodies.is_empty() {
                    return Err("Select the bodies for the component".into());
                }
                let prefix = if group { "Subassembly" } else { "Component" };
                let mut name = prefix.to_owned();
                let mut i = 2;
                while a
                    .component_structure
                    .definitions
                    .iter()
                    .any(|d| d.name == name)
                {
                    name = format!("{prefix} {i}");
                    i += 1;
                }
                request = Some((
                    "assembly_create_component",
                    json!({"name":name,"body_ids":bodies,"absorb_promoted_bodies":true}),
                ));
            }
            Command::Add(child) => {
                let id = state
                    .definition
                    .ok_or("Choose a reusable component definition")?;
                let d = a
                    .component_structure
                    .definitions
                    .iter()
                    .find(|d| d.id.0 == id)
                    .ok_or("The definition no longer exists")?;
                let parent = if child {
                    Some(selected.ok_or("Select the parent component")?)
                } else {
                    None
                };
                request = Some((
                    "assembly_create_occurrence",
                    json!({"component_id":id,"name":d.name,"parent_occurrence_id":parent}),
                ));
            }
            Command::Parent(id, parent) => {
                find(&a, id)?;
                if let Some(parent) = parent {
                    find(&a, parent)?;
                    if descendants(&a, id).contains(&parent) {
                        return Err("A component cannot be its own ancestor".into());
                    }
                }
                state.parents_open = false;
                request = Some((
                    "assembly_update_occurrence",
                    json!({"occurrence":{"id":id,"parent_occurrence_id":parent}}),
                ));
            }
            Command::Rename(id, definition) | Command::ApplyTransform(id, definition) => {
                let o = find(&a, id)?;
                let draft = state
                    .draft
                    .as_ref()
                    .filter(|d| d.occurrence == id)
                    .ok_or("The component editor changed")?;
                let value = if matches!(command, Command::Rename(..)) {
                    let name = if definition {
                        &draft.definition_name
                    } else {
                        &draft.name
                    };
                    if name.trim().is_empty() {
                        return Err("Enter a component name".into());
                    }
                    json!({"name":name.trim()})
                } else {
                    let transform = if definition {
                        &draft.origin
                    } else {
                        &draft.placement
                    };
                    json!({if definition {"local_coordinate_system"} else {"local_pose"}:transform.value(state.units)?})
                };
                let mut patch = value;
                patch["id"] = json!(if definition { o.component_id.0 } else { id });
                request = Some((
                    if definition {
                        "assembly_update_component"
                    } else {
                        "assembly_update_occurrence"
                    },
                    json!({if definition {"component"} else {"occurrence"}:patch}),
                ));
            }
            Command::Show(_)
            | Command::Edit(..)
            | Command::Joint(_)
            | Command::Inspect(_)
            | Command::Motion(_)
            | Command::Study(_) => {
                unreachable!()
            }
        }
        if let Some((op, args)) = request {
            if native_viewport::interface_geometry(world)
                .active_sketch
                .is_some()
            {
                return Err("Finish the active sketch before editing the assembly".into());
            }
            if feature::panel(world).is_some() || joint::active(world) {
                return Err("Finish or cancel the open feature before editing the assembly".into());
            }
            mutation(
                world,
                engine,
                bridge,
                &receipt.owner,
                receipt.revision,
                op,
                args,
            )
        } else {
            Ok(json!({"updated":true}))
        }
    })();
    world.insert_resource(state);
    result
}
pub(crate) fn scroll(world: &mut World, delta: f32) -> bool {
    let Some(mut state) = world.get_resource_mut::<Browser>().filter(|s| s.enabled) else {
        return false;
    };
    state.scroll = (state.scroll + delta).clamp(0., state.max_scroll);
    true
}
pub(crate) fn synchronize(
    world: &mut World,
    services: &NativeServices,
    owner: &DocumentContext,
    revision: u64,
    bounds: InterfaceRect,
) -> Result<(), String> {
    let mut state = world.remove_resource::<Browser>().unwrap_or_default();
    let result = (|| {
        state.widgets.begin();
        if !state.enabled {
            // Hidden browsers still observe document replacement/closure. Keep
            // editable fields until the normal owner refresh, but do not pin
            // the previous document's immutable assembly snapshot indefinitely.
            if state.owner.as_ref() != Some(owner) {
                state.assembly = None;
            }
            state.widgets.finish(world);
            return Ok(());
        }
        if state.owner.as_ref() != Some(owner) {
            state.owner = Some(owner.clone());
            state.assembly = None;
            state.collapsed.clear();
            state.draft = None;
            state.scroll = 0.;
            state.parents_open = false;
            state.definitions_open = false;
            state.inspect = default();
            state.motion = default();
            state.studies = default();
        }
        if state.assembly.is_none() || state.revision != revision {
            let a = services
                .bridge
                .with_native_document_receipt(&services.engine, owner, |r| {
                    if r != revision {
                        return Err("Assembly revision changed during refresh".into());
                    }
                    document(&services.engine)
                })?;
            state.units = services.engine.document_units();
            if !a
                .component_structure
                .definitions
                .iter()
                .any(|d| Some(d.id.0) == state.definition)
            {
                state.definition = a.component_structure.definitions.first().map(|d| d.id.0);
            }
            state.motion.changed(&a);
            state.studies.changed(&a);
            state.assembly = Some(Arc::new(a));
            state.revision = revision;
            state.draft = None;
            state.inspect.changed();
        }
        let a = state.assembly.as_ref().unwrap().clone();
        let selected = native_viewport::interface_view(world)
            .2
            .selected_occurrence_id;
        if state.draft.as_ref().map(|d| d.occurrence) != selected {
            state.draft = selected.and_then(|id| find(&a, id).ok()).and_then(|o| {
                a.component_structure
                    .definitions
                    .iter()
                    .find(|d| d.id == o.component_id)
                    .map(|d| Draft::new(o, d, state.units))
            });
            state.parents_open = false;
        }
        panel::paint(world, &mut state, &a, bounds)?;
        state.widgets.finish(world);
        Ok(())
    })();
    world.insert_resource(state);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_bridge::native_interface::tests::Fixture;
    #[test]
    fn hidden_browser_releases_foreign_snapshot_but_preserves_same_owner() {
        let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
        let fixture = Fixture::new();
        let owner = fixture.owner();
        let services = NativeServices {
            engine: fixture.engine.clone(),
            bridge: fixture.bridge.clone(),
        };
        let snapshot = Arc::new(document(&fixture.engine).unwrap());
        let retained = Arc::downgrade(&snapshot);
        let mut world = World::new();
        world.insert_resource(Browser {
            owner: Some(owner.clone()),
            assembly: Some(snapshot),
            ..default()
        });
        synchronize(&mut world, &services, &owner, 0, default()).unwrap();
        assert!(retained.upgrade().is_some());
        let mut replacement = owner.clone();
        replacement.epoch += 1;
        synchronize(&mut world, &services, &replacement, 0, default()).unwrap();
        assert!(retained.upgrade().is_none());
        // The ordinary enabled refresh still owns draft/selection retirement.
        assert_eq!(world.resource::<Browser>().owner.as_ref(), Some(&owner));
    }

    #[test]
    fn placement_edits_preserve_exact_rotations_accept_units_and_reject_invalid_values() {
        let q = DQuat::from_euler(EulerRot::ZYX, 1.234, 0.17, -0.3).to_array();
        let mut draft = TransformDraft::new(
            AssemblyTransformDto {
                translation: [10., 20., 30.],
                rotation: q,
            },
            UnitSystem::Mm,
        );
        draft.translation[0].set_text("2 in".into());
        let value = draft.value(UnitSystem::Mm).unwrap();
        assert_eq!(
            value.rotation, q,
            "Editing translation must not round-trip the saved quaternion"
        );
        assert_eq!(value.translation, [50.8, 20., 30.]);
        for invalid in ["NaN", "1/0", "1e50"] {
            draft.translation[0].set_text(invalid.into());
            assert!(draft.value(UnitSystem::Mm).is_err());
        }
        draft.translation[0].set_text("2".into());
        assert_eq!(draft.value(UnitSystem::Cm).unwrap().translation[0], 20.);
    }
    #[test]
    fn assembly_structure_mutations_have_exact_undo_without_deleting_source_features() {
        let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
        let fixture = Fixture::new();
        let owner = fixture.owner();
        for (op, args) in [
            ("sketch_begin", json!({"type":"origin_plane","plane":"xy"})),
            (
                "sketch_add_rectangle",
                json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":20.,"y":10.},"ctrl_held":true}),
            ),
            ("sketch_finish", json!({})),
            (
                "solid_extrude",
                json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":10.}}),
            ),
        ] {
            fixture
                .bridge
                .apply_native_mutation(&fixture.engine, &owner, op, &args, || Ok(()))
                .unwrap();
        }
        let source = fixture.engine.document_snapshot().features.len();
        let export = || {
            parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap()
        };
        let mut app = native_viewport::interface_scene_fixture();
        refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
        for (op, args) in [
            (
                "assembly_create_component",
                json!({"name":"Bracket","body_ids":[1],"absorb_promoted_bodies":true}),
            ),
            (
                "assembly_create_occurrence",
                json!({"component_id":2,"name":"Second bracket"}),
            ),
            (
                "assembly_update_component",
                json!({"component":{"id":2,"name":"Renamed part"}}),
            ),
            (
                "assembly_update_occurrence",
                json!({"occurrence":{"id":2,"name":"Fixed bracket","visible":false,"local_pose":{"translation":[40.,0.,0.],"rotation":[0.,0.,0.,1.]}}}),
            ),
            (
                "assembly_set_occurrence_grounded",
                json!({"occurrence_id":2,"grounded":true}),
            ),
            ("assembly_duplicate_occurrence", json!({"occurrence_id":2})),
        ] {
            let owner = fixture.owner();
            let before = export();
            let receipt = fixture
                .bridge
                .native_document_receipt(&fixture.engine, &owner)
                .unwrap();
            mutation(
                app.world_mut(),
                &fixture.engine,
                &fixture.bridge,
                &owner,
                receipt.revision,
                op,
                args,
            )
            .unwrap();
            let after = export();
            assert_ne!(after, before, "{op}");
            assert_eq!(fixture.engine.document_snapshot().features.len(), source);
            let undo = fixture
                .bridge
                .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
                .unwrap();
            let mut before_model: Value = serde_json::from_str(before.as_str().unwrap()).unwrap();
            let after_model: Value = serde_json::from_str(after.as_str().unwrap()).unwrap();
            let undo_model: Value = serde_json::from_str(export().as_str().unwrap()).unwrap();
            let allocator = "/assembly/component_structure/next_occurrence_id";
            assert_eq!(
                undo_model.pointer(allocator),
                after_model.pointer(allocator),
                "Undo {op} must preserve the occurrence allocation floor"
            );
            assert!(
                undo_model.pointer(allocator).unwrap().as_u64().unwrap()
                    >= before_model.pointer(allocator).unwrap().as_u64().unwrap()
            );
            *before_model.pointer_mut(allocator).unwrap() =
                undo_model.pointer(allocator).unwrap().clone();
            assert_eq!(undo_model, before_model, "Undo {op}");
            fixture
                .bridge
                .apply_native_history(&fixture.engine, &undo.context, true, || Ok(()))
                .unwrap();
            assert_eq!(export(), after, "Redo {op}");
        }
    }
}
