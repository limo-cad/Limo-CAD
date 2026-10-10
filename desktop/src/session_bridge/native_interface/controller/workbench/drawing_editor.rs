//! Native forms over the existing drawing document and guarded engine commands.
use super::*;
use limo_cad_interface::{ChoiceOption, ControlInput};
use limo_cad_sketch::DrawingDocumentDto;
use model::{Draft, Selection};
mod model;
mod panel;
mod tables;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Annotation(u64, drawing_authoring::Command),
    SheetChoice,
    ViewChoice,
    Sheet,
    Table(tables::Table),
    TableRow(tables::Table),
    NewRow(tables::Table),
    DeleteRow,
    Edit(Selection, usize),
    Apply,
    Reset,
    AutoLayout,
    Fields(i32),
}
#[derive(Resource, Default)]
struct Editor {
    owner: Option<DocumentContext>,
    revision: u64,
    document: Arc<DrawingDocumentDto>,
    draft: Option<Draft>,
    pending_selection: Option<Selection>,
    page: usize,
    message: String,
    widgets: Widgets,
}
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

fn options(rows: Vec<(u64, String)>) -> Vec<ChoiceOption> {
    rows.into_iter()
        .map(|(id, label)| ChoiceOption {
            value: id.to_string(),
            label,
            disabled: false,
        })
        .collect()
}
fn choose(options: &[ChoiceOption], current: &str, input: &ControlInput) -> Result<String, String> {
    cam::choose(options, current, input).map_err(|_| "Choose an available drawing value".into())
}
fn native(command: Command) -> NativeCommand {
    NativeCommand::Drawing(command)
}
pub(crate) fn guard_ribbon_edit(world: &World, operation: &str) -> Result<(), String> {
    if operation.starts_with("drawing_") && workspace(world) == Workspace::Drawing {
        drawing_authoring::guard(world)?;
        guard_sheet_edit(world)?;
    }
    Ok(())
}
/// Tab/file transitions must not discard an unapplied sheet/view draft.
/// Keep this owner-scoped so an unrelated window cannot block the transition.
pub(crate) fn guard_document_switch(world: &World, owner: &DocumentContext) -> Result<(), String> {
    if world.get_resource::<Editor>().is_some_and(|editor| {
        super::same_document(editor.owner.as_ref(), owner)
            && editor.draft.as_ref().is_some_and(Draft::dirty)
    }) {
        return Err("Apply or reset the drawing edit before switching documents".into());
    }
    Ok(())
}

pub(super) fn guard_sheet_edit(world: &World) -> Result<(), String> {
    if world
        .get_resource::<Editor>()
        .and_then(|e| e.draft.as_ref())
        .is_some_and(Draft::dirty)
    {
        return Err("Apply or reset the drawing edit first".into());
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
    if let Command::Annotation(serial, command) = command {
        return drawing_authoring::reduce(world, handle, engine, bridge, action, *serial, command);
    }
    drawing_authoring::guard(world)?;
    let receipt = bridge.native_document_receipt(engine, &action.context)?;
    bridge
        .with_native_document_owner(engine, &action.context, || handle.validate_action(action))?;
    let mut editor = world
        .remove_resource::<Editor>()
        .ok_or("Open the Drawing workspace")?;
    let result = (|| {
        if workspace(world) != Workspace::Drawing
            || editor.owner.as_ref() != Some(&receipt.owner)
            || editor.revision != receipt.revision
        {
            return Err("Drawing changed; use the refreshed controls".into());
        }
        if let Command::Edit(selection, index) = command {
            let draft = editor
                .draft
                .as_mut()
                .filter(|d| d.selection == *selection)
                .ok_or("Drawing selection changed")?;
            let field = draft
                .fields
                .get(*index)
                .ok_or("Drawing field was removed")?;
            let text = if matches!(field.kind, model::Kind::Body) {
                choose(
                    &panel::body_options(world, &field.text),
                    &field.text,
                    &action.control.input,
                )?
            } else if let model::Kind::Choice(values) = field.kind {
                choose(
                    &values
                        .iter()
                        .map(|(value, label)| ChoiceOption {
                            value: (*value).into(),
                            label: (*label).into(),
                            disabled: false,
                        })
                        .collect::<Vec<_>>(),
                    &field.text,
                    &action.control.input,
                )?
            } else {
                match &action.control.input {
                    ControlInput::SetValue(text) => text.clone(),
                    ControlInput::Key(_) => return Ok(json!({"handled":true})),
                    input if super::super::super::is_activation(input) => {
                        return Ok(json!({"focused":true}));
                    }
                    _ => return Err("Edit the drawing field with text".into()),
                }
            };
            draft.set(*index, text)?;
            editor.message.clear();
            return Ok(json!({"changed":true}));
        }
        let dirty = editor.draft.as_ref().is_some_and(Draft::dirty);
        if dirty
            && matches!(
                command,
                Command::SheetChoice
                    | Command::ViewChoice
                    | Command::Sheet
                    | Command::AutoLayout
                    | Command::Table(_)
                    | Command::TableRow(_)
                    | Command::NewRow(_)
                    | Command::DeleteRow
            )
        {
            return Err("Apply or reset the drawing edit first".into());
        }
        let mut request = None;
        match command {
            Command::TableRow(table) => {
                let sheet = editor
                    .document
                    .active_sheet_id
                    .ok_or("Create a sheet first")?;
                let current = editor
                    .draft
                    .as_ref()
                    .and_then(|d| match d.selection {
                        Selection::Row(_, t, id) if t == *table => Some(id.to_string()),
                        _ => None,
                    })
                    .unwrap_or_else(|| "0".into());
                let row = choose(
                    &options(tables::rows(&editor.document, sheet, *table)?),
                    &current,
                    &action.control.input,
                )?
                .parse::<u64>()
                .map_err(|_| "Invalid drawing row")?;
                editor.draft = Some(Draft::new(
                    &editor.document,
                    if row == 0 {
                        Selection::Table(sheet, *table)
                    } else {
                        Selection::Row(sheet, *table, row)
                    },
                )?);
                editor.page = 0;
            }
            Command::SheetChoice => {
                let id = choose(
                    &options(model::sheets(&editor.document)),
                    &editor
                        .document
                        .active_sheet_id
                        .map_or(String::new(), |id| id.to_string()),
                    &action.control.input,
                )?
                .parse::<u64>()
                .map_err(|_| "Invalid sheet")?;
                editor.pending_selection = Some(Selection::Sheet(id));
                request = Some(("drawing_select_sheet", json!({"sheet_id":id})));
            }
            Command::ViewChoice => {
                let current = editor
                    .draft
                    .as_ref()
                    .and_then(|d| match d.selection {
                        Selection::View(id) => Some(id.to_string()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let id = choose(
                    &options(model::views(&editor.document)),
                    &current,
                    &action.control.input,
                )?
                .parse::<u64>()
                .map_err(|_| "Invalid view")?;
                editor.draft = Some(Draft::new(&editor.document, Selection::View(id))?);
                editor.page = 0;
            }
            _ => {
                if !super::super::super::is_activation(&action.control.input) {
                    return Err("Activate a drawing control".into());
                }
                match command {
                    Command::Table(table) | Command::NewRow(table) => {
                        let sheet = editor
                            .document
                            .active_sheet_id
                            .ok_or("Create a sheet first")?;
                        let selection = if matches!(command, Command::NewRow(_)) {
                            Selection::NewRow(sheet, *table)
                        } else {
                            Selection::Table(sheet, *table)
                        };
                        let mut draft = Draft::new(&editor.document, selection)?;
                        if matches!(selection, Selection::NewRow(_, tables::Table::Bom)) {
                            let existing = tables::sheet(&editor.document, sheet)?;
                            if let Some((id, name)) = world
                                .get_resource::<NativeRenderedDocument>()
                                .and_then(|rendered| {
                                    rendered.bodies.iter().find(|(id, _)| {
                                        !existing.bom.iter().any(|row| {
                                            row.body_id.is_some_and(|body| body.0 == *id)
                                        })
                                    })
                                })
                            {
                                for (path, text) in
                                    [("/body_id", id.to_string()), ("/description", name.clone())]
                                {
                                    let index = draft
                                        .fields
                                        .iter()
                                        .position(|field| field.path == path)
                                        .ok_or("BOM field was removed")?;
                                    draft.set(index, text)?;
                                }
                            }
                        }
                        editor.draft = Some(draft);
                        editor.page = 0;
                    }
                    Command::DeleteRow => {
                        let selection =
                            editor.draft.as_ref().ok_or("Select a table row")?.selection;
                        let next = tables::delete(&editor.document, selection)?;
                        let (sheet, table) =
                            tables::context(selection).ok_or("Select a table row")?;
                        editor.pending_selection = Some(Selection::Table(sheet, table));
                        request = Some((
                            "drawing_set_document",
                            serde_json::to_value(next).map_err(|e| e.to_string())?,
                        ));
                    }
                    Command::Sheet => {
                        let id = editor
                            .document
                            .active_sheet_id
                            .ok_or("Create a sheet first")?;
                        editor.draft = Some(Draft::new(&editor.document, Selection::Sheet(id))?);
                        editor.page = 0;
                    }
                    Command::Apply => {
                        if !dirty {
                            return Ok(json!({"unchanged":true}));
                        }
                        let draft = editor.draft.as_ref().ok_or("Select a sheet or view")?;
                        editor.pending_selection = Some(draft.committed_selection());
                        request = Some(if matches!(draft.selection, Selection::View(_)) {
                            (
                                "drawing_update_view",
                                serde_json::to_value(draft.view_edit(&editor.document)?)
                                    .map_err(|e| e.to_string())?,
                            )
                        } else {
                            (
                                "drawing_set_document",
                                serde_json::to_value(draft.apply(&editor.document)?)
                                    .map_err(|e| e.to_string())?,
                            )
                        });
                    }
                    Command::Reset => {
                        editor.draft = editor
                            .draft
                            .as_ref()
                            .map(|d| {
                                Draft::new(
                                    &editor.document,
                                    match d.selection {
                                        Selection::NewRow(sheet, table) => {
                                            Selection::Table(sheet, table)
                                        }
                                        selection => selection,
                                    },
                                )
                            })
                            .transpose()?;
                    }
                    Command::AutoLayout => {
                        let next = model::auto_layout(
                            &editor.document,
                            native_viewport::interface_geometry(world).scene,
                        )?;
                        editor.pending_selection = next
                            .sheets
                            .iter()
                            .find(|s| Some(s.id) == next.active_sheet_id)
                            .and_then(|s| s.views.first())
                            .map(|v| Selection::View(v.id));
                        request = Some((
                            "drawing_set_document",
                            serde_json::to_value(next).map_err(|e| e.to_string())?,
                        ));
                    }
                    Command::Fields(delta) => {
                        editor.page = editor.page.saturating_add_signed(*delta as isize)
                    }
                    _ => unreachable!(),
                }
            }
        }
        editor.message.clear();
        if let Some((operation, args)) = request {
            if worker::available(world) {
                let expected_owner = receipt.owner.clone();
                return worker::enqueue_operation(
                    world,
                    receipt.owner.clone(),
                    receipt.revision,
                    operation.into(),
                    args,
                    move |world, services, result| {
                        let result = result.inspect_err(|error| {
                            if let Some(mut editor) = world.get_resource_mut::<Editor>() {
                                if editor.owner.as_ref() == Some(&expected_owner) {
                                    editor.message = error.clone();
                                }
                            }
                        })?;
                        Ok(finish_mutation(
                            &services.engine,
                            &services.bridge,
                            world,
                            operation,
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
                || handle.validate_action(action),
            )?;
            return Ok(finish_mutation(engine, bridge, world, operation, result));
        }
        Ok(json!({"updated":true}))
    })();
    if let Err(error) = &result {
        editor.message = error.clone();
    }
    world.insert_resource(editor);
    result
}

pub(super) fn synchronize(
    world: &mut World,
    camera: Entity,
    services: &NativeServices,
    owner: &DocumentContext,
    (height, side): (f32, f32),
    active: bool,
    state: &Workbench,
) -> Result<(), String> {
    let mut editor = world.remove_resource::<Editor>().unwrap_or_default();
    editor.widgets.begin();
    let result = (|| {
        if !active {
            if editor
                .owner
                .as_ref()
                .is_some_and(|previous| previous != owner)
                && (super::same_document(editor.owner.as_ref(), owner)
                    || !editor.draft.as_ref().is_some_and(Draft::dirty))
            {
                // Retire replaced incarnations and clean inactive snapshots.
                // A different live tab must never silently discard a dirty
                // draft; ordinary tab/file switches are guarded before enqueue.
                editor.owner = None;
                editor.document = Arc::default();
                editor.draft = None;
                editor.pending_selection = None;
                editor.message.clear();
            }
            return Ok(());
        }
        let receipt = services
            .bridge
            .native_document_receipt(&services.engine, owner)?;
        if editor.owner.as_ref() != Some(owner) || editor.revision != receipt.revision {
            let same_document = super::same_document(editor.owner.as_ref(), owner);
            if editor.owner.as_ref() != Some(owner) {
                editor.pending_selection = None;
            }
            let selected = same_document
                .then(|| {
                    editor
                        .pending_selection
                        .take()
                        .or_else(|| editor.draft.as_ref().map(|d| d.selection))
                })
                .flatten();
            editor.document = state
                .paper_document
                .as_ref()
                .filter(|(previous, _)| previous == &receipt)
                .map(|(_, document)| Arc::clone(document))
                .unwrap_or_else(|| Arc::new(services.engine.drawing_snapshot()));
            editor.owner = Some(owner.clone());
            editor.revision = receipt.revision;
            let selected = selected
                .filter(|s| match s {
                    Selection::Sheet(id) => Some(*id) == editor.document.active_sheet_id,
                    Selection::View(id) => model::views(&editor.document)
                        .iter()
                        .any(|(view, _)| view == id),
                    selection => tables::context(*selection).is_some_and(|(id, _)| {
                        Some(id) == editor.document.active_sheet_id
                            && !matches!(selection, Selection::NewRow(..))
                            && tables::record(&editor.document, *selection).is_ok()
                    }),
                })
                .or_else(|| editor.document.active_sheet_id.map(Selection::Sheet));
            editor.draft = selected
                .map(|s| Draft::new(&editor.document, s))
                .transpose()?;
            editor.page = 0;
            editor.message.clear();
        }
        if drawing_authoring::owns_panel(world) {
            Ok(())
        } else {
            panel::paint(world, camera, &mut editor, height, side)
        }
    })();
    editor.widgets.finish(world);
    world.insert_resource(editor);
    result
}
