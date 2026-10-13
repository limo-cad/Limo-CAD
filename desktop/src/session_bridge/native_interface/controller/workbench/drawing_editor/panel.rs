use super::*;
use limo_cad_interface::{Field, KeyChord};

pub(super) fn body_options(world: &World, current: &str) -> Vec<ChoiceOption> {
    let mut choices = vec![ChoiceOption {
        value: String::new(),
        label: "No body".into(),
        disabled: false,
    }];
    if let Some(rendered) = world.get_resource::<NativeRenderedDocument>() {
        choices.extend(rendered.bodies.iter().map(|(id, name)| ChoiceOption {
            value: id.to_string(),
            label: name.clone(),
            disabled: false,
        }));
    }
    if !choices.iter().any(|c| c.value == current) {
        choices.push(ChoiceOption {
            value: current.into(),
            label: format!("Missing body {current}"),
            disabled: true,
        });
    }
    choices
}

fn button(
    (world, camera, widgets): (&mut World, Entity, &mut Widgets),
    key: &str,
    label: &str,
    command: Command,
    bounds: Node,
    disabled: bool,
) -> Result<(), String> {
    let mut control = InterfaceControl::button("drawing/document", label);
    control.disabled = disabled;
    widgets.button(
        world,
        camera,
        key,
        control,
        None,
        native(command),
        bounds,
        None,
        46,
    )?;
    Ok(())
}
fn choice(
    (world, camera, widgets): (&mut World, Entity, &mut Widgets),
    key: &str,
    label: &str,
    command: Command,
    value: String,
    options: Vec<ChoiceOption>,
    bounds: Node,
) -> Result<(), String> {
    let caption = options
        .iter()
        .find(|o| o.value == value)
        .map(|o| o.label.as_str())
        .unwrap_or("Choose…")
        .to_owned();
    let mut control = InterfaceControl::button("drawing/document", label);
    control.role = "combobox".into();
    control.disabled = options.is_empty();
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
    control.field = Field::Choice { value, options };
    widgets.button(
        world,
        camera,
        key,
        control,
        Some(&caption),
        native(command),
        bounds,
        None,
        46,
    )?;
    Ok(())
}
pub(super) fn paint(
    world: &mut World,
    camera: Entity,
    editor: &mut Editor,
    height: f32,
    side: f32,
) -> Result<(), String> {
    let theme = crate::native_viewport::ui::theme(world);
    let width = side.max(248.);
    let bottom = (height - 66.).max(280.);
    editor.widgets.panel(
        world,
        camera,
        "drawing-editor-panel",
        rect(0., 112., width, bottom - 112.),
        theme.panel,
        44,
    );
    editor.widgets.text(
        world,
        camera,
        "drawing-editor-title",
        rect(12., 121., width - 24., 18.),
        "Drawing document",
        12.,
        45,
    );
    choice(
        (world, camera, &mut editor.widgets),
        "drawing-sheet-choice",
        "Sheet",
        Command::SheetChoice,
        editor
            .document
            .active_sheet_id
            .map_or(String::new(), |id| id.to_string()),
        options(model::sheets(&editor.document)),
        rect(10., 146., width - 20., 28.),
    )?;
    let view = editor
        .draft
        .as_ref()
        .and_then(|d| match d.selection {
            Selection::View(id) => Some(id.to_string()),
            _ => None,
        })
        .unwrap_or_default();
    choice(
        (world, camera, &mut editor.widgets),
        "drawing-view-choice",
        "View",
        Command::ViewChoice,
        view,
        options(model::views(&editor.document)),
        rect(10., 179., width - 20., 28.),
    )?;
    let active_sheet = editor
        .document
        .sheets
        .iter()
        .find(|s| Some(s.id) == editor.document.active_sheet_id);
    button(
        (world, camera, &mut editor.widgets),
        "drawing-sheet-edit",
        "Sheet setup",
        Command::Sheet,
        rect(10., 212., (width - 26.) / 2., 28.),
        active_sheet.is_none(),
    )?;
    button(
        (world, camera, &mut editor.widgets),
        "drawing-auto-layout",
        "Auto-layout",
        Command::AutoLayout,
        rect(width / 2. + 3., 212., (width - 26.) / 2., 28.),
        active_sheet.is_none_or(|s| !s.views.is_empty()),
    )?;
    for (table, key, label, x) in [
        (
            tables::Table::Revisions,
            "drawing-revisions",
            "Revisions",
            10.,
        ),
        (
            tables::Table::Bom,
            "drawing-bom",
            "Bill of materials",
            width / 2. + 3.,
        ),
    ] {
        button(
            (world, camera, &mut editor.widgets),
            key,
            label,
            Command::Table(table),
            rect(x, 245., (width - 26.) / 2., 28.),
            active_sheet.is_none(),
        )?;
    }
    let Some(draft) = &editor.draft else {
        editor.widgets.text(
            world,
            camera,
            "drawing-editor-empty",
            rect(12., 286., width - 24., 54.),
            "Create a sheet to edit its setup and place views.",
            11.,
            45,
        );
        return Ok(());
    };
    let field_top = if let Some((sheet, table)) = tables::context(draft.selection) {
        let current = match draft.selection {
            Selection::Row(_, _, id) => id.to_string(),
            Selection::NewRow(..) => "new".into(),
            _ => "0".into(),
        };
        let mut rows = options(tables::rows(&editor.document, sheet, table)?);
        if matches!(draft.selection, Selection::NewRow(..)) {
            rows.push(ChoiceOption {
                value: "new".into(),
                label: match table {
                    tables::Table::Revisions => "New revision",
                    tables::Table::Bom => "New item",
                }
                .into(),
                disabled: true,
            });
        }
        choice(
            (world, camera, &mut editor.widgets),
            "drawing-table-row",
            match table {
                tables::Table::Revisions => "Revision row",
                tables::Table::Bom => "BOM row",
            },
            Command::TableRow(table),
            current,
            rows,
            rect(10., 278., width - 20., 28.),
        )?;
        button(
            (world, camera, &mut editor.widgets),
            "drawing-table-add",
            "Add row",
            Command::NewRow(table),
            rect(10., 311., (width - 26.) / 2., 28.),
            draft.dirty(),
        )?;
        button(
            (world, camera, &mut editor.widgets),
            "drawing-table-delete",
            "Delete row",
            Command::DeleteRow,
            rect(width / 2. + 3., 311., (width - 26.) / 2., 28.),
            !matches!(draft.selection, Selection::Row(..)) || draft.read_only() || draft.dirty(),
        )?;
        349.
    } else {
        283.
    };
    let visible: Vec<_> = draft
        .fields
        .iter()
        .enumerate()
        .filter(|(_, f)| {
            f.path != "/tolerance_note/custom"
                || draft.fields.iter().any(|preset| {
                    preset.path == "/tolerance_note/preset" && preset.text == "custom"
                })
        })
        .collect();
    let page_size = (((bottom - field_top - 117.) / 46.).floor() as usize).clamp(1, 9);
    editor.page = editor.page.min(visible.len().saturating_sub(1) / page_size);
    let mut pending_text_edit = false;
    for (position, (index, field)) in visible
        .iter()
        .copied()
        .enumerate()
        .skip(editor.page * page_size)
        .take(page_size)
    {
        let y = field_top + (position % page_size) as f32 * 46.;
        editor.widgets.text(
            world,
            camera,
            &format!("drawing-label-{index}"),
            rect(12., y, width - 24., 16.),
            field.label,
            10.,
            45,
        );
        let key = format!("drawing-field-{}", field.path);
        let command = Command::Edit(draft.selection, index);
        if matches!(field.kind, model::Kind::Body) && !draft.read_only() {
            let options = body_options(world, &field.text);
            choice(
                (world, camera, &mut editor.widgets),
                &key,
                field.label,
                command,
                field.text.clone(),
                options,
                rect(10., y + 16., width - 20., 28.),
            )?;
        } else if let Some(values) = match field.kind {
            model::Kind::Choice(values) if !draft.read_only() => Some(values),
            _ => None,
        } {
            choice(
                (world, camera, &mut editor.widgets),
                &key,
                field.label,
                command,
                field.text.clone(),
                values
                    .iter()
                    .map(|(value, label)| ChoiceOption {
                        value: (*value).into(),
                        label: (*label).into(),
                        disabled: false,
                    })
                    .collect(),
                rect(10., y + 16., width - 20., 28.),
            )?;
        } else {
            let mut control = InterfaceControl::button("drawing/document", field.label);
            control.field = Field::Text {
                value: field.text.clone(),
                read_only: draft.read_only(),
                selection: None,
            };
            let entity = editor.widgets.button(
                world,
                camera,
                &key,
                control,
                None,
                native(command),
                rect(10., y + 16., width - 20., 28.),
                None,
                46,
            )?;
            pending_text_edit |= interface_shell::fields::has_uncommitted_edit(world, entity);
        }
    }
    // Native text stays local until blur. Keep Apply available so its first
    // click can queue that field commit before the existing guarded command.
    let dirty = draft.dirty() || pending_text_edit;
    let y = field_top + 2. + page_size as f32 * 46.;
    if visible.len() > page_size {
        button(
            (world, camera, &mut editor.widgets),
            "drawing-fields-prev",
            "Previous fields",
            Command::Fields(-1),
            rect(10., y, (width - 26.) / 2., 26.),
            editor.page == 0,
        )?;
        button(
            (world, camera, &mut editor.widgets),
            "drawing-fields-next",
            "More fields",
            Command::Fields(1),
            rect(width / 2. + 3., y, (width - 26.) / 2., 26.),
            (editor.page + 1) * page_size >= visible.len(),
        )?;
    }
    button(
        (world, camera, &mut editor.widgets),
        "drawing-apply",
        "Apply",
        Command::Apply,
        rect(10., y + 32., (width - 26.) / 2., 28.),
        draft.read_only() || !dirty,
    )?;
    button(
        (world, camera, &mut editor.widgets),
        "drawing-reset",
        if matches!(draft.selection, Selection::NewRow(..)) {
            "Cancel"
        } else {
            "Reset"
        },
        Command::Reset,
        rect(width / 2. + 3., y + 32., (width - 26.) / 2., 28.),
        false,
    )?;
    let message = if editor.message.is_empty() {
        if draft.read_only() {
            "Released revisions are read-only. Add a new revision to record a change."
        } else if dirty {
            "Apply or reset to keep editing another item"
        } else {
            "Paper placement uses millimetres"
        }
    } else {
        &editor.message
    };
    editor.widgets.text(
        world,
        camera,
        "drawing-editor-message",
        rect(12., y + 66., width - 24., 48.),
        message,
        10.,
        45,
    );
    Ok(())
}
