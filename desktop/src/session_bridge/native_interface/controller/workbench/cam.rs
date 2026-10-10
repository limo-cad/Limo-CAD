//! Native editors for the project's existing CAM records. Drafts contain only
//! presentation strings; every commit sends the complete shared document to
//! the same validated command used by MCP.
use super::*;
use limo_cad_cam::{CamDocumentDto, CamOperationDto};
use limo_cad_interface::{ChoiceOption, ControlInput, Field, KeyChord};

mod central;
mod creation;
mod form;
pub(crate) mod geometry_pick;
mod machine;
mod operation_editor;
mod operation_geometry;
mod presets;
mod reorder;
pub(crate) mod reorder_drag;
mod setup;
#[cfg(test)]
mod tests;
mod tool;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tab {
    #[default]
    Setups,
    Tools,
    Toolpaths,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Selection {
    Setup(u64),
    Tool(u64),
    Operation(u64),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    New(Tab),
    Tab(Tab),
    Select(Selection),
    Edit(Selection, usize),
    Toggle(Selection),
    Apply,
    Reset,
    Delete,
    Duplicate,
    Generate,
    SaveProfile,
    Central(central::Command),
    PostStorage(machine::management::Command),
    Move(i32),
    Preset(presets::Command),
    Page(i32),
    Fields(i32),
    PickGeometry(Selection, usize),
}

#[derive(Clone, Copy)]
enum InputKind {
    Name,
    Offset,
    Integer,
    OptionalInteger,
    Length,
    Feed,
    OptionalFeed,
    OptionalLength,
    Number,
    OptionalNumber,
    Choice,
    Boolean,
}
struct DraftField {
    path: String,
    label: String,
    kind: InputKind,
    original: String,
    text: String,
    options: Option<Vec<ChoiceOption>>,
}
struct Draft {
    creation: Option<creation::Context>,
    copied_tool: bool,
    setup: Option<setup::Context>,
    machine: Option<machine::Context>,
    presets: Option<presets::Context>,
    operation_edit: Option<operation_editor::Context>,
    selection: Selection,
    record: Value,
    fields: Vec<DraftField>,
    enabled: Option<bool>,
    original_enabled: Option<bool>,
}
impl Draft {
    fn new(cam: &CamDocumentDto, selection: Selection) -> Result<Self, String> {
        let record = match selection {
            Selection::Setup(id) => serde_json::to_value(cam.setup(id).ok_or("Setup was removed")?),
            Selection::Tool(id) => serde_json::to_value(cam.tool(id).ok_or("Tool was removed")?),
            Selection::Operation(id) => {
                serde_json::to_value(operation(cam, id).ok_or("Toolpath was removed")?)
            }
        }
        .map_err(|e| e.to_string())?;
        let mut descriptors = vec![("/name", "Name", InputKind::Name)];
        match selection {
            Selection::Setup(_) => descriptors.extend([
                (
                    "/work_offset",
                    "Work offset · click to cycle",
                    InputKind::Offset,
                ),
                (
                    "/work_offset_count",
                    "Consecutive work offsets",
                    InputKind::Integer,
                ),
            ]),
            Selection::Tool(_) => descriptors.extend([
                (
                    "/number",
                    "Tool number (optional)",
                    InputKind::OptionalInteger,
                ),
                ("/diameter", "Diameter", InputKind::Length),
                ("/flute_length", "Flute length", InputKind::Length),
                ("/overall_length", "Overall length", InputKind::Length),
                ("/flute_count", "Flutes", InputKind::Integer),
            ]),
            Selection::Operation(_) => descriptors.extend([
                ("/tool_id", "Tool · click to cycle", InputKind::Integer),
                ("/cutting/spindle_rpm", "Spindle (rpm)", InputKind::Integer),
                ("/cutting/feed_xy", "Cutting feed", InputKind::Feed),
                ("/cutting/feed_z", "Plunge feed", InputKind::Feed),
                ("/step_down", "Step down", InputKind::Length),
                ("/step_over", "Step over", InputKind::Length),
            ]),
        }
        let fields = descriptors
            .into_iter()
            .filter_map(|(path, label, kind)| {
                let value = record.pointer(path)?;
                let text = match kind {
                    InputKind::Name | InputKind::Offset => {
                        value.as_str().unwrap_or_default().into()
                    }
                    InputKind::Length | InputKind::Feed => {
                        cam.units.from_mm(value.as_f64()?).to_string()
                    }
                    _ => {
                        if value.is_null() {
                            String::new()
                        } else {
                            value.to_string()
                        }
                    }
                };
                let label = match kind {
                    InputKind::Length => format!("{label} ({})", cam.units.length_label()),
                    InputKind::Feed => format!("{label} ({})", cam.units.feed_label()),
                    _ => label.into(),
                };
                Some(DraftField {
                    path: path.into(),
                    label,
                    kind,
                    original: text.clone(),
                    text,
                    options: None,
                })
            })
            .collect();
        let enabled = record.get("enabled").and_then(Value::as_bool);
        let mut draft = Self {
            creation: None,
            copied_tool: false,
            setup: None,
            machine: None,
            presets: None,
            operation_edit: None,
            selection,
            record,
            fields,
            enabled,
            original_enabled: enabled,
        };
        if matches!(selection, Selection::Tool(_)) {
            tool::extend(&mut draft, cam, false)?;
            presets::extend_tool(&mut draft, cam.units)?;
        }
        presets::extend_operation(&mut draft, cam);
        Ok(draft)
    }
    fn dirty(&self) -> bool {
        self.creation.is_some()
            || self.copied_tool
            || self.enabled != self.original_enabled
            || self
                .fields
                .iter()
                .any(|f| f.text != f.original && !f.path.starts_with("/native/ui/"))
    }
    fn edited(&self, cam: &CamDocumentDto) -> Result<CamDocumentDto, String> {
        let next = self.edited_without_validation(cam)?;
        next.validate_for_editing()?;
        Ok(next)
    }
    fn edited_without_validation(&self, cam: &CamDocumentDto) -> Result<CamDocumentDto, String> {
        let mut record = self.record.clone();
        for field in self
            .fields
            .iter()
            .filter(|f| f.text != f.original && !f.path.starts_with("/native/"))
        {
            let text = field.text.trim();
            let value = match field.kind {
                InputKind::Name => {
                    if text.is_empty() {
                        return Err("Enter a name".into());
                    }
                    json!(text)
                }
                InputKind::Offset => json!(text.to_ascii_lowercase()),
                InputKind::Choice => json!(text),
                InputKind::Boolean => json!(text
                    .parse::<bool>()
                    .map_err(|_| format!("Choose {}", field.label))?),
                InputKind::OptionalInteger if text.is_empty() => Value::Null,
                InputKind::OptionalLength | InputKind::OptionalFeed | InputKind::OptionalNumber
                    if text.is_empty() =>
                {
                    Value::Null
                }
                InputKind::Integer | InputKind::OptionalInteger => json!(text
                    .parse::<u64>()
                    .map_err(|_| format!("{} must be a whole number", field.label))?),
                InputKind::Length
                | InputKind::Feed
                | InputKind::OptionalFeed
                | InputKind::OptionalLength
                | InputKind::Number
                | InputKind::OptionalNumber => {
                    let number = text
                        .parse::<f64>()
                        .map_err(|_| format!("Enter a number for {}", field.label))?;
                    let number = if matches!(
                        field.kind,
                        InputKind::Length
                            | InputKind::Feed
                            | InputKind::OptionalLength
                            | InputKind::OptionalFeed
                    ) {
                        cam.units.to_mm(number)
                    } else {
                        number
                    };
                    if !number.is_finite() {
                        return Err(format!("{} must be finite", field.label));
                    }
                    json!(number)
                }
            };
            *record
                .pointer_mut(&field.path)
                .ok_or("CAM field was removed")? = value;
        }
        if let Some(enabled) = self.enabled {
            record["enabled"] = json!(enabled);
        }
        if self.setup.is_some() {
            setup::apply(self, &mut record, cam)?;
        }
        if self.machine.is_some() {
            machine::apply(self, &mut record, cam.units)?;
        }
        if matches!(self.selection, Selection::Tool(_)) {
            tool::apply(self, &mut record, cam.units)?;
            presets::apply_tool(self, &mut record, cam.units)?;
        }
        presets::apply_operation(self, &mut record, cam)?;
        let mut next = cam.clone();
        if self.operation_edit.is_some() {
            operation_editor::apply(self, &mut record, &mut next)?;
        }
        replace_record_unvalidated(&next, self.selection, record)
    }
}

fn operation(cam: &CamDocumentDto, id: u64) -> Option<&CamOperationDto> {
    cam.setups
        .iter()
        .flat_map(|s| &s.operations)
        .find(|o| o.id() == id)
}
fn draft_for(world: &World, cam: &CamDocumentDto, selection: Selection) -> Result<Draft, String> {
    let mut draft = Draft::new(cam, selection)?;
    let geometry = native_viewport::interface_geometry(world);
    setup::extend(&mut draft, cam, geometry.scene, geometry.finished_sketches)?;
    machine::extend(&mut draft, cam, machine::snapshot(world))?;
    operation_editor::extend_shared(&mut draft, cam, geometry.scene, geometry.finished_sketches)?;
    Ok(draft)
}

fn choices(cam: &CamDocumentDto, path: &str) -> Option<Vec<ChoiceOption>> {
    match path {
        "/tool_id" => Some(
            cam.tools
                .iter()
                .map(|tool| ChoiceOption {
                    value: tool.id.to_string(),
                    label: match tool.number {
                        Some(number) => format!("T{number} · {}", tool.name),
                        None => tool.name.clone(),
                    },
                    disabled: false,
                })
                .collect(),
        ),
        "/work_offset" => Some(
            (54..=59)
                .map(|n| ChoiceOption {
                    value: format!("g{n}"),
                    label: format!("G{n}"),
                    disabled: false,
                })
                .collect(),
        ),
        _ => None,
    }
}

pub(crate) fn choose(
    options: &[ChoiceOption],
    current: &str,
    input: &ControlInput,
) -> Result<String, String> {
    let options: Vec<_> = options.iter().filter(|option| !option.disabled).collect();
    if options.is_empty() {
        return Err("No CAM choices are available".into());
    }
    if let ControlInput::SetValue(value) = input {
        return options
            .iter()
            .find(|o| o.value == *value)
            .map(|o| o.value.clone())
            .ok_or("Choose one of the available CAM values".into());
    }
    let selected = options.iter().position(|o| o.value == current);
    let index = selected.unwrap_or(0);
    let next = match input {
        ControlInput::Key(key) if !key.ctrl && !key.meta && !key.alt && !key.shift => {
            match key.key.as_str() {
                "ArrowUp" | "ArrowLeft" => index.checked_sub(1).unwrap_or(options.len() - 1),
                "ArrowDown" | "ArrowRight" | "Enter" | " " | "Space" => {
                    selected.map_or(0, |i| (i + 1) % options.len())
                }
                "Home" => 0,
                "End" => options.len() - 1,
                _ => return Err("Use arrow keys to choose a CAM value".into()),
            }
        }
        input if super::super::super::is_activation(input) => {
            selected.map_or(0, |i| (i + 1) % options.len())
        }
        _ => return Err("Choose a CAM value".into()),
    };
    Ok(options[next].value.clone())
}
fn replace_record_unvalidated(
    cam: &CamDocumentDto,
    selection: Selection,
    record: Value,
) -> Result<CamDocumentDto, String> {
    let mut next = cam.clone();
    match selection {
        Selection::Setup(id) => {
            *next
                .setups
                .iter_mut()
                .find(|s| s.id == id)
                .ok_or("Setup was removed")? =
                serde_json::from_value(record).map_err(|e| e.to_string())?
        }
        Selection::Tool(id) => {
            *next
                .tools
                .iter_mut()
                .find(|t| t.id == id)
                .ok_or("Tool was removed")? =
                serde_json::from_value(record).map_err(|e| e.to_string())?
        }
        Selection::Operation(id) => {
            *next
                .setups
                .iter_mut()
                .flat_map(|s| &mut s.operations)
                .find(|o| o.id() == id)
                .ok_or("Toolpath was removed")? =
                serde_json::from_value(record).map_err(|e| e.to_string())?
        }
    }
    Ok(next)
}

fn remove(cam: &CamDocumentDto, selection: Selection) -> Result<CamDocumentDto, String> {
    let mut next = cam.clone();
    let removed: Vec<_> = match selection {
        Selection::Setup(id) => {
            let ids = cam
                .setup(id)
                .ok_or("Setup was removed")?
                .operations
                .iter()
                .map(|o| o.id())
                .collect();
            next.setups.retain(|s| s.id != id);
            if next.active_setup_id == Some(id) {
                next.active_setup_id = next.setups.first().map(|s| s.id);
            }
            ids
        }
        Selection::Tool(id) => {
            if cam
                .setups
                .iter()
                .flat_map(|s| &s.operations)
                .any(|o| o.tool_id() == id)
            {
                return Err("This tool is used by a toolpath; change its tool first".into());
            }
            next.tools.retain(|t| t.id != id);
            vec![]
        }
        Selection::Operation(id) => {
            for setup in &mut next.setups {
                setup.operations.retain(|o| o.id() != id);
            }
            vec![id]
        }
    };
    next.toolpath_generations
        .retain(|r| !removed.contains(&r.operation_id));
    next.height_expressions
        .retain(|r| !removed.contains(&r.operation_id));
    next.linking.retain(|r| !removed.contains(&r.operation_id));
    next.validate_for_editing()?;
    Ok(next)
}

fn next_id(counter: u64, ids: impl Iterator<Item = u64>) -> Result<u64, String> {
    let id = ids
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or("CAM identities exhausted")?
        .max(counter)
        .max(1);
    id.checked_add(1).ok_or("CAM identities exhausted")?;
    Ok(id)
}
fn copy_name(name: &str, names: impl Iterator<Item = String>) -> String {
    let names: Vec<_> = names.collect();
    let mut copy = format!("{name} (copy)");
    let mut n = 2;
    while names.contains(&copy) {
        copy = format!("{name} (copy {n})");
        n += 1;
    }
    copy
}
fn copy_intent(next: &mut CamDocumentDto, source: &CamDocumentDto, ids: &[(u64, u64)]) {
    for (old, new) in ids {
        for item in source
            .height_expressions
            .iter()
            .filter(|r| r.operation_id == *old)
        {
            let mut item = item.clone();
            item.operation_id = *new;
            next.height_expressions.push(item);
        }
        for item in source.linking.iter().filter(|r| r.operation_id == *old) {
            let mut item = item.clone();
            item.operation_id = *new;
            next.linking.push(item);
        }
    }
}
fn duplicate(
    cam: &CamDocumentDto,
    selection: Selection,
) -> Result<(CamDocumentDto, Selection), String> {
    let mut next = cam.clone();
    let mut ids = vec![];
    let selection = match selection {
        Selection::Tool(id) => {
            let mut tool = cam.tool(id).ok_or("Tool was removed")?.clone();
            tool.id = next_id(cam.next_tool_id, cam.tools.iter().map(|t| t.id))?;
            tool.name = copy_name(&tool.name, cam.tools.iter().map(|t| t.name.clone()));
            tool.number = None;
            next.next_tool_id = tool.id + 1;
            let id = tool.id;
            next.tools.push(tool);
            Selection::Tool(id)
        }
        Selection::Setup(id) => {
            let index = cam
                .setups
                .iter()
                .position(|s| s.id == id)
                .ok_or("Setup was removed")?;
            let mut setup = cam.setups[index].clone();
            setup.id = next_id(cam.next_setup_id, cam.setups.iter().map(|s| s.id))?;
            setup.name = copy_name(&setup.name, cam.setups.iter().map(|s| s.name.clone()));
            let mut op_id = next_id(
                cam.next_operation_id,
                cam.setups
                    .iter()
                    .flat_map(|s| &s.operations)
                    .map(|o| o.id()),
            )?;
            for op in &mut setup.operations {
                let old = op.id();
                let mut value = serde_json::to_value(&*op).map_err(|e| e.to_string())?;
                value["id"] = json!(op_id);
                *op = serde_json::from_value(value).map_err(|e| e.to_string())?;
                ids.push((old, op_id));
                op_id = op_id.checked_add(1).ok_or("CAM identities exhausted")?;
            }
            next.next_operation_id = op_id;
            next.next_setup_id = setup.id + 1;
            next.active_setup_id = Some(setup.id);
            let id = setup.id;
            next.setups.insert(index + 1, setup);
            Selection::Setup(id)
        }
        Selection::Operation(id) => {
            let setup_index = cam
                .setups
                .iter()
                .position(|s| s.operations.iter().any(|o| o.id() == id))
                .ok_or("Toolpath was removed")?;
            let setup = &cam.setups[setup_index];
            let index = setup.operations.iter().position(|o| o.id() == id).unwrap();
            let new_id = next_id(
                cam.next_operation_id,
                cam.setups
                    .iter()
                    .flat_map(|s| &s.operations)
                    .map(|o| o.id()),
            )?;
            let mut value =
                serde_json::to_value(&setup.operations[index]).map_err(|e| e.to_string())?;
            value["id"] = json!(new_id);
            value["name"] = json!(copy_name(
                setup.operations[index].name(),
                setup.operations.iter().map(|o| o.name().into())
            ));
            next.setups[setup_index].operations.insert(
                index + 1,
                serde_json::from_value(value).map_err(|e| e.to_string())?,
            );
            next.next_operation_id = new_id + 1;
            next.active_setup_id = Some(setup.id);
            ids.push((id, new_id));
            Selection::Operation(new_id)
        }
    };
    copy_intent(&mut next, cam, &ids);
    next.validate_for_editing()?;
    Ok((next, selection))
}

#[derive(Resource, Default)]
struct Editor {
    owner: Option<DocumentContext>,
    revision: u64,
    cam: CamDocumentDto,
    tab: Tab,
    draft: Option<Draft>,
    pending_selection: Option<Selection>,
    pending_notice: Option<String>,
    page: usize,
    field_page: usize,
    widgets: Widgets,
    message: String,
}
/// Inactive workspaces do not refresh the CAM editor. Release its retained
/// scene explicitly when its document closes or clean geometry is evicted.
/// Unapplied drafts remain an intentional ownership boundary under pressure.
pub(super) fn retire_document(world: &mut World, owner: &DocumentContext, closed: bool) {
    let retire = world.get_resource::<Editor>().is_some_and(|editor| {
        super::same_document(editor.owner.as_ref(), owner)
            && (closed || !editor.draft.as_ref().is_some_and(Draft::dirty))
    });
    if retire {
        let mut editor = world.remove_resource::<Editor>().unwrap();
        editor.widgets.begin();
        editor.widgets.finish(world);
    }
}

pub(super) fn selected(world: &World) -> Option<Selection> {
    world
        .get_resource::<Editor>()?
        .draft
        .as_ref()
        .map(|draft| draft.selection)
        .filter(|selection| {
            !matches!(
                selection,
                Selection::Setup(0) | Selection::Tool(0) | Selection::Operation(0)
            )
        })
}
pub(super) fn editing_dirty(world: &World) -> bool {
    world
        .get_resource::<Editor>()
        .and_then(|editor| editor.draft.as_ref())
        .is_some_and(Draft::dirty)
}

pub(crate) fn modal(world: &World) -> Option<&'static str> {
    central::modal(world)
}
pub(crate) fn escape(world: &mut World) {
    central::escape(world);
}
pub(crate) fn awaiting(world: &World) -> bool {
    central::awaiting(world) || machine::management::awaiting(world)
}
pub(crate) fn caption(world: &World) -> Option<String> {
    central::caption(world).or_else(|| {
        world
            .get_resource::<Editor>()
            .and_then(|editor| editor.draft.as_ref())
            .filter(|draft| machine::management_visible(draft))
            .map(|_| machine::management::caption(world))
    })
}
pub(super) fn synchronize_library(
    world: &mut World,
    camera: Entity,
    services: &NativeServices,
    owner: &DocumentContext,
    width: f32,
    height: f32,
    active: bool,
) -> Result<(), String> {
    central::synchronize(world, camera, services, owner, width, height, active)
}
fn rows(cam: &CamDocumentDto, tab: Tab) -> Vec<(Selection, String)> {
    match tab {
        Tab::Setups => cam
            .setups
            .iter()
            .map(|s| (Selection::Setup(s.id), s.name.clone()))
            .collect(),
        Tab::Tools => cam
            .tools
            .iter()
            .map(|t| {
                (
                    Selection::Tool(t.id),
                    match t.number {
                        Some(number) => format!("T{number} · {}", t.name),
                        None => t.name.clone(),
                    },
                )
            })
            .collect(),
        Tab::Toolpaths => cam
            .setups
            .iter()
            .flat_map(|s| {
                s.operations.iter().map(move |o| {
                    (
                        Selection::Operation(o.id()),
                        format!("{} / {}", s.name, o.name()),
                    )
                })
            })
            .collect(),
    }
}
fn ensure_current(editor: &Editor, owner: &DocumentContext, revision: u64) -> Result<(), String> {
    if editor.owner.as_ref() != Some(owner) || editor.revision != revision {
        return Err("CAM changed; use the refreshed controls".into());
    }
    Ok(())
}

pub(crate) fn reduce(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    engine: &AppState,
    bridge: &SessionBridgeState,
    action: &NativeInterfaceAction,
    command: &Command,
) -> Result<Value, String> {
    if let Command::Central(command) = command {
        return central::execute(world, handle, engine, bridge, action, command);
    }
    let receipt = bridge.native_document_receipt(engine, &action.context)?;
    bridge
        .with_native_document_owner(engine, &action.context, || handle.validate_action(action))?;
    if matches!(command, Command::Apply) {
        geometry_pick::settled(world)?;
    }
    if !matches!(
        command,
        Command::PickGeometry(..) | Command::Page(_) | Command::Fields(_)
    ) {
        geometry_pick::cancel(world, handle);
    }
    let mut editor = world
        .remove_resource::<Editor>()
        .ok_or("Open the CAM workspace")?;
    let result = (|| {
        ensure_current(&editor, &receipt.owner, receipt.revision)?;
        if workspace(world) != Workspace::Cam {
            return Err("Open the CAM workspace".into());
        }
        if let Command::Edit(selection, index) = command {
            let draft = editor
                .draft
                .as_mut()
                .filter(|d| d.selection == *selection)
                .ok_or("The CAM editor changed")?;
            let field = draft
                .fields
                .get_mut(*index)
                .ok_or("CAM field was removed")?;
            let options = field
                .options
                .clone()
                .or_else(|| {
                    draft
                        .creation
                        .as_ref()
                        .and_then(|creation| creation.choices(&editor.cam, &field.path))
                })
                .or_else(|| choices(&editor.cam, &field.path));
            if let Some(options) = options {
                if options.is_empty() {
                    return Err(match field.path.as_str() {
                        "/body_id" => "No model bodies are available; create a solid first",
                        "/setup_id" => "No setups are available; create a setup first",
                        "/native/ui/cutting_profile" => {
                            "No cutting presets are available; use Add preset"
                        }
                        _ => "No project tools are available; create a tool first",
                    }
                    .into());
                }
                field.text = choose(&options, &field.text, &action.control.input)?;
                let path = field.path.clone();
                if matches!(
                    path.as_str(),
                    "/native/ui/operation_section"
                        | "/native/ui/setup_section"
                        | "/native/ui/tool_section"
                ) {
                    editor.field_page = 0;
                }
                machine::changed(draft, editor.cam.units, &path)?;
                if path == "/native/ui/setup_section" && form::text(draft, &path)? == "machine" {
                    machine::reload(world)?;
                }
                if path == "/native/ui/setup_section" && machine::management_visible(draft) {
                    machine::management::ensure_loaded(world);
                }
                if matches!(draft.selection, Selection::Tool(_)) {
                    tool::changed(draft, &path);
                    presets::changed_tool(draft, editor.cam.units, &path)?;
                }
                if draft.creation.is_some() {
                    creation::seed_choices(draft, &editor.cam)?;
                }
                operation_editor::changed(draft, &editor.cam, &path)?;
                presets::changed_operation(draft, &editor.cam, &path)?;
                editor.message.clear();
                return Ok(json!({"changed":true}));
            }
            return match &action.control.input {
                ControlInput::SetValue(text) => {
                    let field = draft
                        .fields
                        .get_mut(*index)
                        .ok_or("CAM field was removed")?;
                    field.text = text.clone();
                    let path = field.path.clone();
                    operation_editor::changed(draft, &editor.cam, &path)?;
                    presets::changed_tool(draft, editor.cam.units, &path)?;
                    editor.message.clear();
                    Ok(json!({"changed":true}))
                }
                input if super::super::super::is_activation(input) => Ok(json!({"focused":true})),
                ControlInput::Key(_) => Ok(json!({"handled":true})),
                _ => Err("Edit the CAM field with text".into()),
            };
        }
        if !super::super::super::is_activation(&action.control.input) {
            return Err("Activate a CAM control".into());
        }
        let dirty = editor.draft.as_ref().is_some_and(Draft::dirty);
        if dirty
            && matches!(
                command,
                Command::New(_)
                    | Command::Tab(_)
                    | Command::Select(_)
                    | Command::Delete
                    | Command::Duplicate
                    | Command::Generate
                    | Command::SaveProfile
                    | Command::Move(_)
                    | Command::PostStorage(_)
            )
        {
            return Err("Apply or reset the current CAM edit first".into());
        }
        let mut request = None;
        match *command {
            Command::PickGeometry(selection, index) => {
                let draft = editor
                    .draft
                    .as_ref()
                    .filter(|draft| draft.selection == selection)
                    .ok_or("The geometry editor changed")?;
                if !draft.fields.get(index).is_some_and(|field| {
                    geometry_pick::is_button(&field.path)
                        && operation_editor::visible(draft, &field.path)
                        && setup::visible(draft, &field.path)
                        && machine::visible(draft, &field.path)
                }) {
                    return Err("The geometry picker control changed".into());
                }
                let path = &draft.fields[index].path;
                let linking_pick = operation_editor::linking_points::picking::is_button(path);
                let height_pick = operation_editor::heights::picking::is_button(path);
                let value = if linking_pick || height_pick {
                    geometry_pick::toggle_target(world, handle, &receipt, &editor, path)?
                } else {
                    geometry_pick::toggle(world, handle, &receipt, &editor)?
                };
                editor.message = if geometry_pick::active(world) {
                    if matches!(draft.selection, Selection::Setup(_)) {
                        "Click a WCS origin handle. Escape ends picking; Apply saves the setup."
                    } else if height_pick {
                        "Click a highlighted height handle. Escape ends picking; Apply saves the draft."
                    } else if linking_pick {
                        "Click a highlighted linking position. Escape ends picking; Apply saves the draft."
                    } else if matches!(draft.record["kind"].as_str(), Some("drill" | "thread")) {
                        "Click a cylindrical wall to toggle a hole. Escape ends picking; Apply saves the draft."
                    } else {
                        "Click an edge. Alt-click toggles one edge. Escape ends picking; Apply saves the draft."
                    }
                } else {
                    "Geometry is staged. Apply saves the draft."
                }.into();
                return Ok(value);
            }
            Command::New(tab) => {
                if feature::panel(world).is_some()
                    || native_viewport::interface_geometry(world)
                        .active_sketch
                        .is_some()
                {
                    return Err("Finish the current modeling edit before creating CAM items".into());
                }
                let context = creation::Context::shared(
                    native_viewport::interface_geometry(world).scene,
                    &editor.cam,
                )?
                .with_sketches(native_viewport::interface_geometry(world).finished_sketches);
                editor.draft = Some(creation::draft(tab, &editor.cam, context));
                editor.tab = tab;
                editor.field_page = 0;
                editor.message.clear();
            }
            Command::Tab(tab) => {
                editor.tab = tab;
                editor.page = 0;
                editor.field_page = 0;
                editor.draft = rows(&editor.cam, tab)
                    .first()
                    .map(|(s, _)| draft_for(world, &editor.cam, *s))
                    .transpose()?;
                editor.message.clear();
            }
            Command::Select(selection) => {
                editor.draft = Some(draft_for(world, &editor.cam, selection)?);
                editor.field_page = 0;
                editor.message.clear();
            }
            Command::Page(delta) => editor.page = editor.page.saturating_add_signed(delta as isize),
            Command::Fields(delta) => {
                editor.field_page = editor.field_page.saturating_add_signed(delta as isize)
            }
            Command::Toggle(selection) => {
                let draft = editor
                    .draft
                    .as_mut()
                    .filter(|d| d.selection == selection)
                    .ok_or("The CAM editor changed")?;
                draft.enabled = Some(!draft.enabled.ok_or("Select a toolpath")?);
            }
            Command::Preset(command) => {
                let draft = editor
                    .draft
                    .as_mut()
                    .ok_or("Open a tool's cutting presets")?;
                presets::edit_tool(draft, editor.cam.units, command)?;
                editor.field_page = 0;
                editor.message.clear();
            }
            Command::Reset => {
                let mut next = if editor.draft.as_ref().is_some_and(|d| d.creation.is_some()) {
                    None
                } else {
                    editor
                        .draft
                        .as_ref()
                        .map(|d| draft_for(world, &editor.cam, d.selection))
                        .transpose()?
                };
                if let Some(next) = next.as_mut() {
                    operation_editor::retain_section(editor.draft.as_ref(), next);
                    machine::retain_section(editor.draft.as_ref(), next);
                    presets::retain(editor.draft.as_ref(), next, editor.cam.units);
                }
                editor.draft = next;
                editor.message.clear();
            }
            Command::Apply => {
                if !dirty {
                    return Ok(json!({"unchanged":true}));
                }
                let draft = editor.draft.as_ref().ok_or("Select a CAM item")?;
                let created_tool = (draft.creation.is_some() || draft.copied_tool)
                    && matches!(draft.selection, Selection::Tool(_));
                let next = if draft.copied_tool {
                    let (next, selected) = central::create_copy(draft, &editor.cam)?;
                    editor.pending_selection = Some(selected);
                    next
                } else if draft.creation.is_some() {
                    let (next, selected) = creation::create(draft, &editor.cam)?;
                    editor.pending_selection = Some(selected);
                    next
                } else {
                    draft.edited(&editor.cam)?
                };
                if created_tool && worker::available(world) {
                    let Some(Selection::Tool(id)) = editor.pending_selection else {
                        return Err("Created tool selection is unavailable".into());
                    };
                    return central::create_project_tool(
                        world,
                        receipt.clone(),
                        editor.cam.clone(),
                        next.tool(id).ok_or("Created tool is unavailable")?.clone(),
                    );
                }
                request = Some((
                    "cam_set_document",
                    serde_json::to_value(next).map_err(|e| e.to_string())?,
                ));
            }
            Command::Delete => {
                let next = remove(
                    &editor.cam,
                    editor.draft.as_ref().ok_or("Select a CAM item")?.selection,
                )?;
                request = Some((
                    "cam_set_document",
                    serde_json::to_value(next).map_err(|e| e.to_string())?,
                ));
            }
            Command::Duplicate => {
                if let Selection::Tool(id) =
                    editor.draft.as_ref().ok_or("Select a CAM item")?.selection
                {
                    editor.draft = Some(central::copy_tool(&editor.cam, id)?);
                    editor.field_page = 0;
                    editor.message.clear();
                    return Ok(json!({"changed":true,"unsaved_copy":true}));
                }
                let (next, selected) = duplicate(
                    &editor.cam,
                    editor.draft.as_ref().ok_or("Select a CAM item")?.selection,
                )?;
                editor.pending_selection = Some(selected);
                request = Some((
                    "cam_set_document",
                    serde_json::to_value(next).map_err(|e| e.to_string())?,
                ));
            }
            Command::Generate => match editor
                .draft
                .as_ref()
                .ok_or("Select a setup or toolpath")?
                .selection
            {
                Selection::Setup(id) => {
                    request = Some(("cam_regenerate_setup", json!({"setup_id":id})))
                }
                Selection::Operation(id) => {
                    request = Some(("cam_regenerate_operation", json!({"operation_id":id})))
                }
                Selection::Tool(_) => return Err("Select a setup or toolpath to generate".into()),
            },
            Command::SaveProfile => {
                let Selection::Setup(id) = editor.draft.as_ref().ok_or("Select a setup")?.selection
                else {
                    return Err("Select a setup machine to save".into());
                };
                let machine = editor
                    .cam
                    .setup(id)
                    .and_then(|setup| setup.machine.as_ref())
                    .ok_or("Assign and apply a setup machine first")?;
                machine::save_profile(world, machine.clone())?;
            }
            Command::Move(delta) => {
                let selected = editor
                    .draft
                    .as_ref()
                    .ok_or("Select a setup or toolpath")?
                    .selection;
                let next = reorder::step(&editor.cam, selected, delta)?;
                editor.pending_selection = Some(selected);
                request = Some((
                    "cam_set_document",
                    serde_json::to_value(next).map_err(|error| error.to_string())?,
                ));
            }
            Command::PostStorage(command) => return machine::management::execute(world, command),
            Command::Edit(..) | Command::Central(..) => unreachable!(),
        }
        if let Some((operation, args)) = request {
            submit(world, engine, bridge, &receipt, operation, args, || {
                handle.validate_action(action)
            })
        } else {
            Ok(json!({"updated":true}))
        }
    })();
    if let Err(error) = &result {
        editor.message = error.clone();
    }
    world.insert_resource(editor);
    result
}

/// Form edits, keyboard ordering and completed pointer drops enter the same
/// receipt-checked mutation queue and project history boundary.
fn submit(
    world: &mut World,
    engine: &AppState,
    bridge: &SessionBridgeState,
    receipt: &workspace::DocumentReceipt,
    operation: &str,
    args: Value,
    guard: impl FnOnce() -> Result<(), String>,
) -> Result<Value, String> {
    if worker::available(world) {
        let label = operation.to_owned();
        return worker::enqueue_operation(
            world,
            receipt.owner.clone(),
            receipt.revision,
            operation.into(),
            args,
            move |world, services, result| {
                let result = result.inspect_err(|error| {
                    if let Some(mut editor) = world.get_resource_mut::<Editor>() {
                        editor.message = error.clone();
                    }
                })?;
                Ok(finish_mutation(
                    &services.engine,
                    &services.bridge,
                    world,
                    &label,
                    result,
                ))
            },
        );
    }
    let result = bridge.apply_native_mutation_at(
        engine,
        &receipt.owner,
        receipt.revision,
        operation,
        &args,
        guard,
    )?;
    Ok(finish_mutation(engine, bridge, world, operation, result))
}

fn button(
    (widgets, world, camera): (&mut Widgets, &mut World, Entity),
    key: &str,
    label: &str,
    command: Command,
    bounds: Node,
    disabled: bool,
    selected: Option<bool>,
) -> Result<(), String> {
    let mut control = InterfaceControl::button("cam/document", label);
    if matches!(
        command,
        Command::Select(Selection::Setup(_) | Selection::Operation(_))
    ) {
        control
            .owned_keys
            .extend(["ArrowUp", "ArrowDown"].map(|key| KeyChord {
                key: key.into(),
                alt: true,
                ..default()
            }));
    }
    control.disabled = disabled;
    control.selected = selected;
    widgets.button(
        world,
        camera,
        key,
        control,
        match command {
            Command::Move(-1) => Some("Up"),
            Command::Move(1) => Some("Down"),
            Command::Page(-1) => Some("Prev"),
            _ => None,
        },
        NativeCommand::Cam(command),
        bounds,
        None,
        46,
    )?;
    Ok(())
}

pub(super) fn ribbon(
    world: &mut World,
    camera: Entity,
    x: f32,
    widgets: &mut Widgets,
) -> Result<(), String> {
    let selected = world
        .get_resource::<Editor>()
        .map_or(Tab::Setups, |s| s.tab);
    for (index, (tab, label)) in [
        (Tab::Setups, "Setups"),
        (Tab::Tools, "Project tools"),
        (Tab::Toolpaths, "Toolpaths"),
    ]
    .into_iter()
    .enumerate()
    {
        button(
            (widgets, world, camera),
            &format!("cam-tab-{index}"),
            label,
            Command::Tab(tab),
            rect(x + index as f32 * 112., 36., 106., 32.),
            false,
            Some(selected == tab),
        )?;
    }
    widgets.text(
        world,
        camera,
        "cam-ribbon-help",
        rect(x + 4., 79., 530., 18.),
        "Edit project CAM · Apply changes, then Generate toolpaths",
        11.,
        31,
    );
    Ok(())
}

pub(super) fn synchronize(
    world: &mut World,
    camera: Entity,
    services: &NativeServices,
    owner: &DocumentContext,
    height: f32,
    side: f32,
    active: bool,
) -> Result<(), String> {
    let mut editor = world.remove_resource::<Editor>().unwrap_or_default();
    editor.widgets.begin();
    let result = (|| {
        machine::management::poll(world);
        if !active {
            return Ok(());
        }
        if machine::poll(world) {
            if let Some(draft) = editor.draft.as_mut() {
                machine::refresh_library(draft, &editor.cam, machine::snapshot(world));
            }
        }
        let receipt = services
            .bridge
            .native_document_receipt(&services.engine, owner)?;
        if editor.owner.as_ref() != Some(owner) || editor.revision != receipt.revision {
            let same_incarnation = editor.owner.as_ref() == Some(owner);
            let same_document = super::same_document(editor.owner.as_ref(), owner);
            if !same_incarnation {
                editor.pending_selection = None;
            }
            let selection = same_document
                .then(|| {
                    editor
                        .pending_selection
                        .take()
                        .or_else(|| editor.draft.as_ref().map(|d| d.selection))
                })
                .flatten();
            editor.cam = services.engine.cam_document_snapshot();
            editor.owner = Some(owner.clone());
            editor.revision = receipt.revision;
            if !same_document {
                editor.tab = Tab::Setups;
                editor.page = 0;
            }
            let selection = selection
                .filter(|s| Draft::new(&editor.cam, *s).is_ok())
                .or_else(|| rows(&editor.cam, editor.tab).first().map(|(s, _)| *s));
            if let Some(selected) = selection {
                if let Some(index) = rows(&editor.cam, editor.tab)
                    .iter()
                    .position(|(s, _)| *s == selected)
                {
                    editor.page = index / 3;
                }
            }
            let mut next_draft = selection
                .map(|s| draft_for(world, &editor.cam, s))
                .transpose()?;
            if same_document {
                if let Some(next) = next_draft.as_mut() {
                    operation_editor::retain_section(editor.draft.as_ref(), next);
                    machine::retain_section(editor.draft.as_ref(), next);
                    presets::retain(editor.draft.as_ref(), next, editor.cam.units);
                }
            }
            editor.draft = next_draft;
            editor.message = if same_document {
                editor.pending_notice.take().unwrap_or_default()
            } else {
                editor.pending_notice = None;
                String::new()
            };
        }
        let theme = crate::native_viewport::ui::theme(world);
        let w = side.max(248.);
        let bottom = (height - 66.).max(280.);
        editor.widgets.panel(
            world,
            camera,
            "cam-panel",
            rect(0., 112., w, bottom - 112.),
            theme.panel,
            44,
        );
        let items = rows(&editor.cam, editor.tab);
        let count = items.len();
        editor.page = editor.page.min(count.saturating_sub(1) / 3);
        if editor.tab == Tab::Tools {
            button(
                (&mut editor.widgets, world, camera),
                "cam-central-library",
                "Central library",
                Command::Central(central::Command::Open),
                rect(10., 118., (w - 26.) / 2., 26.),
                false,
                None,
            )?;
        } else {
            editor.widgets.text(
                world,
                camera,
                "cam-heading",
                rect(12., 122., w - 144., 18.),
                &format!(
                    "{} {}",
                    count,
                    match editor.tab {
                        Tab::Setups => "setups",
                        Tab::Tools => "project tools",
                        Tab::Toolpaths => "toolpaths",
                    }
                ),
                12.,
                45,
            );
        }
        button(
            (&mut editor.widgets, world, camera),
            "cam-new",
            match editor.tab {
                Tab::Setups => "New setup",
                Tab::Tools => "New project tool",
                Tab::Toolpaths => "New toolpath",
            },
            Command::New(editor.tab),
            if editor.tab == Tab::Tools {
                rect(w / 2. + 3., 118., (w - 26.) / 2., 26.)
            } else {
                rect(w - 140., 118., 130., 26.)
            },
            false,
            None,
        )?;
        for (i, (selection, label)) in items.iter().enumerate().skip(editor.page * 3).take(3) {
            button(
                (&mut editor.widgets, world, camera),
                &format!("cam-item-{i}"),
                label,
                Command::Select(*selection),
                rect(10., 146. + (i % 3) as f32 * 31., w - 20., 28.),
                false,
                Some(
                    editor
                        .draft
                        .as_ref()
                        .is_some_and(|d| d.selection == *selection),
                ),
            )?;
        }
        reorder_drag::paint(world, camera, &mut editor.widgets, w);
        if items.is_empty() {
            editor.widgets.text(
                world,
                camera,
                "cam-empty",
                rect(12., 150., w - 24., 44.),
                match editor.tab {
                    Tab::Setups => "No setups in this document",
                    Tab::Tools => "No project tools in this document",
                    Tab::Toolpaths => "No toolpaths in this document",
                },
                11.,
                45,
            );
        }
        button(
            (&mut editor.widgets, world, camera),
            "cam-prev",
            "Previous",
            Command::Page(-1),
            rect(
                10.,
                241.,
                if editor.tab == Tab::Tools { 90. } else { 56. },
                26.,
            ),
            editor.page == 0,
            None,
        )?;
        button(
            (&mut editor.widgets, world, camera),
            "cam-next",
            "Next",
            Command::Page(1),
            if editor.tab == Tab::Tools {
                rect(w - 100., 241., 90., 26.)
            } else {
                rect(w - 66., 241., 56., 26.)
            },
            (editor.page + 1) * 3 >= count,
            None,
        )?;
        if editor.tab != Tab::Tools {
            let selection = editor.draft.as_ref().map(|draft| draft.selection);
            for (index, (label, delta)) in
                [("Move up", -1), ("Move down", 1)].into_iter().enumerate()
            {
                let disabled = editor.draft.as_ref().is_none_or(Draft::dirty)
                    || !selection
                        .is_some_and(|selection| reorder::can_step(&editor.cam, selection, delta));
                let bw = (w - 144.) / 2.;
                button(
                    (&mut editor.widgets, world, camera),
                    &format!("cam-move-{index}"),
                    label,
                    Command::Move(delta),
                    rect(70. + index as f32 * (bw + 4.), 241., bw, 26.),
                    disabled,
                    None,
                )?;
            }
        }
        let Some(draft) = &editor.draft else {
            return Ok(());
        };
        let dirty = draft.dirty();
        let selected = draft.selection;
        let visible_fields: Vec<_> = draft
            .fields
            .iter()
            .enumerate()
            .filter(|(_, field)| {
                tool::visible(draft, &field.path)
                    && presets::visible_tool(draft, &field.path)
                    && presets::visible_operation(&field.path)
                    && setup::visible(draft, &field.path)
                    && machine::visible(draft, &field.path)
                    && operation_editor::visible(draft, &field.path)
            })
            .collect();
        let page_size = (((bottom - 415.) / 46.).floor() as usize).clamp(1, 8);
        editor.field_page = editor
            .field_page
            .min(visible_fields.len().saturating_sub(1) / page_size);
        for (position, (index, field)) in visible_fields
            .iter()
            .copied()
            .enumerate()
            .skip(editor.field_page * page_size)
            .take(page_size)
        {
            let y = 278. + (position % page_size) as f32 * 46.;
            editor.widgets.text(
                world,
                camera,
                &format!("cam-label-{index}"),
                rect(12., y, w - 24., 16.),
                &field.label,
                10.,
                45,
            );
            let mut control = InterfaceControl::button("cam/document", &field.label);
            if geometry_pick::is_button(&field.path) {
                control.selected = Some(
                    geometry_pick::active(world)
                        && geometry_pick::target_matches(world, &field.path),
                );
                control.disabled = geometry_pick::loading(world)
                    || (geometry_pick::active(world)
                        && !geometry_pick::target_matches(world, &field.path));
                editor.widgets.button(
                    world,
                    camera,
                    &format!("cam-field-{}", field.path),
                    control,
                    Some(geometry_pick::label(
                        world,
                        if field.path == setup::picking::BUTTON {
                            "wcs"
                        } else if operation_editor::linking_points::picking::is_button(&field.path)
                        {
                            "linking"
                        } else {
                            draft.record["kind"].as_str().unwrap_or("")
                        },
                        &field.path,
                    )),
                    NativeCommand::Cam(Command::PickGeometry(selected, index)),
                    rect(10., y + 16., w - 20., 28.),
                    None,
                    46,
                )?;
                continue;
            }
            let options = field
                .options
                .clone()
                .or_else(|| {
                    draft
                        .creation
                        .as_ref()
                        .and_then(|creation| creation.choices(&editor.cam, &field.path))
                })
                .or_else(|| choices(&editor.cam, &field.path));
            let caption = if let Some(options) = options {
                let caption = options
                    .iter()
                    .find(|o| o.value == field.text)
                    .map(|o| o.label.clone())
                    .unwrap_or_else(|| "Choose…".into());
                control.role = "combobox".into();
                control.owned_keys = [
                    "ArrowUp",
                    "ArrowDown",
                    "ArrowLeft",
                    "ArrowRight",
                    "Home",
                    "End",
                ]
                .map(KeyChord::plain)
                .into();
                control.disabled = options.is_empty();
                control.field = Field::Choice {
                    value: field.text.clone(),
                    options,
                };
                Some(caption)
            } else {
                control.field = Field::Text {
                    value: field.text.clone(),
                    read_only: false,
                    selection: None,
                };
                None
            };
            editor.widgets.button(
                world,
                camera,
                &format!("cam-field-{}", field.path),
                control,
                caption.as_deref(),
                NativeCommand::Cam(Command::Edit(selected, index)),
                rect(10., y + 16., w - 20., 28.),
                None,
                46,
            )?;
        }
        if machine::management_visible(draft) {
            machine::management::ensure_loaded(world);
            machine::management::paint(
                world,
                camera,
                &mut editor.widgets,
                (10., 332., w - 20., (bottom - 340.).max(130.)),
                |command| NativeCommand::Cam(Command::PostStorage(command)),
            )?;
            return Ok(());
        }
        let y = 280. + page_size as f32 * 46.;
        if visible_fields.len() > page_size {
            button(
                (&mut editor.widgets, world, camera),
                "cam-fields-prev",
                "Previous fields",
                Command::Fields(-1),
                rect(10., y, (w - 26.) / 2., 26.),
                editor.field_page == 0,
                None,
            )?;
            button(
                (&mut editor.widgets, world, camera),
                "cam-fields-next",
                "More fields",
                Command::Fields(1),
                rect(w / 2. + 3., y, (w - 26.) / 2., 26.),
                (editor.field_page + 1) * page_size >= visible_fields.len(),
                None,
            )?;
        }
        let y = y + 30.;
        let bw = (w - 28.) / 3.;
        for (i, (key, label, command, disabled)) in [
            (
                "apply",
                if draft.creation.is_some() || draft.copied_tool {
                    "Create"
                } else {
                    "Apply"
                },
                Command::Apply,
                false,
            ),
            (
                "reset",
                if draft.creation.is_some() || draft.copied_tool {
                    "Cancel"
                } else {
                    "Reset"
                },
                Command::Reset,
                false,
            ),
            (
                "generate",
                "Generate",
                Command::Generate,
                dirty || matches!(selected, Selection::Tool(_)),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            button(
                (&mut editor.widgets, world, camera),
                key,
                label,
                command,
                rect(10. + i as f32 * (bw + 4.), y, bw, 28.),
                disabled,
                None,
            )?;
        }
        if presets::editing(draft) {
            presets::actions(&mut editor.widgets, world, camera, draft, w, y + 32.)?;
        } else {
            for (i, (key, label, command)) in [
                ("duplicate", "Duplicate", Command::Duplicate),
                ("delete", "Delete", Command::Delete),
            ]
            .into_iter()
            .enumerate()
            {
                button(
                    (&mut editor.widgets, world, camera),
                    key,
                    label,
                    command,
                    rect(10. + i as f32 * (bw + 4.), y + 32., bw, 28.),
                    dirty,
                    None,
                )?;
            }
        }
        if let Some(enabled) = draft.enabled {
            button(
                (&mut editor.widgets, world, camera),
                "cam-enabled",
                if enabled { "Enabled" } else { "Disabled" },
                Command::Toggle(selected),
                rect(10. + 2. * (bw + 4.), y + 32., bw, 28.),
                false,
                Some(enabled),
            )?;
        }
        if machine::visible(draft, "/native/machine/source") && draft.machine.is_some() {
            let disabled = dirty || machine::busy(world) || draft.record["machine"].is_null();
            button(
                (&mut editor.widgets, world, camera),
                "cam-save-profile",
                "Save profile",
                Command::SaveProfile,
                rect(10. + 2. * (bw + 4.), y + 32., bw, 28.),
                disabled,
                None,
            )?;
        }
        let setup_preview = if editor.message.is_empty() {
            machine::preview(draft, machine::snapshot(world))
                .or_else(|| setup::preview(draft, &editor.cam))
        } else {
            None
        };
        let message = if let Some(preview) = setup_preview.as_deref() {
            preview
        } else if editor.message.is_empty() && draft.creation.is_some() {
            match selected {
                Selection::Setup(_) => "Box stock from the chosen solid.\nWCS: stock min X / min Y / top; model XYZ.",
                Selection::Tool(_) => "Enter cutter dimensions and cutting data.\nThis tool is saved in the project.",
                Selection::Operation(_) => "Choose geometry and review heights and cutting data.\nCreate saves this toolpath in the selected setup.",
            }
        } else if editor.message.is_empty() {
            if dirty {
                "Unapplied changes"
            } else {
                "Apply edits before generating toolpaths."
            }
        } else {
            &editor.message
        };
        editor.widgets.text(
            world,
            camera,
            "cam-message",
            rect(10., y + 64., w - 20., 36.),
            message,
            10.,
            46,
        );
        Ok(())
    })();
    editor.widgets.finish(world);
    world.insert_resource(editor);
    result
}
