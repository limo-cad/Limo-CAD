use super::super::super::super::chrome::{self, rect};
use super::*;

pub(crate) fn paint_source(
    world: &mut World,
    camera: Entity,
    widgets: &mut chrome::Widgets,
    width: f32,
    height: f32,
    theme: ViewportUiTheme,
    busy: bool,
) -> Result<(), String> {
    let state = &world.resource::<Files>().script;
    let loaded = state.loaded.clone().ok_or("Script source was removed")?;
    let source = state.source.clone();
    let path = state.source_label();
    let generation = state.generation;
    let editor_generation = state.editor_generation;
    let rejected = limit_error(world);
    let pending = state
        .library
        .pending()
        .map(|request| (request.token, request.example.name.clone()));
    let can_run = state.selected(generation).is_ok() && rejected.is_none() && pending.is_none();
    let can_browse_chapters = can_run
        && loaded.inspection["chapters"]
            .as_array()
            .is_some_and(|notes| !notes.is_empty());
    let dirty = state.dirty();
    let has_preview = state.example.is_some_and(|example| example.preview);
    let can_preview = has_preview
        && state
            .example
            .is_some_and(|example| state.source == example.source)
        && rejected.is_none()
        && pending.is_none();
    let can_discard = dirty || rejected.is_some();
    let mut status = rejected
        .clone()
        .or_else(|| state.status.clone())
        .unwrap_or_else(|| {
            "Authored source. Includes are inspected beside this file; Run opens a new design."
                .into()
        });
    let extra = if let Some((_, name)) = &pending {
        status = format!("Opening {name} is waiting for your edits: Save script as, Discard edits, or Cancel opening.\n{status}");
        34.
    } else {
        0.
    };
    let w = (width - 40.).clamp(280., 680.);
    let h = (height - 80.).clamp(310., 780.);
    let x = (width - w - 20.).max(4.);
    let y = 32.;
    widgets.panel(
        world,
        camera,
        "scripts-card",
        rect(x, y, w, h),
        theme.panel.with_alpha(1.),
        60,
    );
    widgets.text(
        world,
        camera,
        "scripts-editor-heading",
        rect(
            x + 12.,
            y + 8.,
            w - if has_preview { 164. } else { 24. },
            24.,
        ),
        if dirty {
            "Script source — unsaved edits"
        } else {
            "Script source"
        },
        15.,
        61,
    );
    if has_preview {
        let mut preview = InterfaceControl::button("document/scripts", "Preview lesson");
        preview.disabled = busy || !can_preview;
        widgets.button(
            world,
            camera,
            "scripts-preview-open",
            preview,
            Some("Preview lesson"),
            NativeCommand::File(FileCommand::ScriptPreview(preview::Action::Open)),
            rect(x + w - 144., y + 8., 132., 24.),
            None,
            61,
        )?;
    }
    let mut provenance = InterfaceControl::button(
        "document/scripts",
        "Script source path and include directory",
    );
    provenance.field = Field::Text {
        value: path,
        selection: None,
        read_only: true,
    };
    widgets.button(
        world,
        camera,
        "scripts-source-path",
        provenance,
        None,
        NativeCommand::File(FileCommand::ScriptPath),
        rect(x + 12., y + 40., w - 24., 26.),
        None,
        61,
    )?;
    let mut control = InterfaceControl::button("document/scripts", "Authored script source");
    control.disabled = busy;
    control.field = Field::Text {
        value: source,
        selection: None,
        read_only: busy,
    };
    let mut bounds = rect(x + 12., y + 74., w - 24., h - 268. - extra);
    bounds.border = UiRect::all(px(1.));
    let entity = widgets.button(
        world,
        camera,
        "scripts-source",
        control,
        None,
        NativeCommand::File(FileCommand::ScriptSource(editor_generation)),
        bounds,
        None,
        61,
    )?;
    fields::multiline::enable(world, entity)?;
    fields::limits::enable(world, entity, loaded.maximum);
    world.resource_mut::<Files>().script.source_entity = Some(entity);
    let font = theme.code_text(world.resource::<ViewportUiAssets>(), 12.);
    if world.get::<TextFont>(entity) != Some(&font) {
        world.entity_mut(entity).insert(font);
    }
    let mut message =
        InterfaceControl::button("document/scripts", "Script validation and save status");
    message.field = Field::Text {
        value: status,
        selection: None,
        read_only: true,
    };
    let message_entity = widgets.button(
        world,
        camera,
        "scripts-source-status",
        message,
        None,
        NativeCommand::File(FileCommand::ScriptPath),
        rect(x + 12., y + h - 186. - extra, w - 24., 66.),
        None,
        61,
    )?;
    fields::multiline::enable(world, message_entity)?;
    if let Some((token, _)) = pending {
        let cancel = InterfaceControl::button("document/scripts", "Cancel opening example");
        widgets.button(
            world,
            camera,
            "scripts-cancel-recipe",
            cancel,
            Some("Cancel opening example"),
            NativeCommand::File(FileCommand::CancelRecipe(token)),
            rect(x + 12., y + h - 146., w - 24., 26.),
            None,
            61,
        )?;
    }
    let gap = 8.;
    launch::paint(world, camera, widgets, x + 12., y + h - 112., w - 24., busy)?;
    let third = (w - 24. - gap * 2.) / 3.;
    for (key, label, command, bounds, disabled) in [
        (
            "scripts-validate",
            "Validate source",
            FileCommand::ValidateScript,
            rect(x + 12., y + h - 76., third, 26.),
            busy || rejected.is_some(),
        ),
        (
            "scripts-save-source",
            "Save script as...",
            FileCommand::SaveScriptAs,
            rect(x + 12. + third + gap, y + h - 76., third, 26.),
            busy || rejected.is_some(),
        ),
        (
            "scripts-discard-source",
            "Discard edits",
            FileCommand::DiscardScriptEdits,
            rect(x + 12. + (third + gap) * 2., y + h - 76., third, 26.),
            busy || !can_discard,
        ),
        (
            "scripts-overview",
            "Back to Scripts",
            FileCommand::ShowScriptSource,
            rect(x + 12., y + h - 40., third, 28.),
            busy,
        ),
        (
            "scripts-chapters",
            "Chapters",
            FileCommand::ScriptChapter(chapters::Action::Open),
            rect(x + 12. + third + gap, y + h - 40., third, 28.),
            busy || !can_browse_chapters,
        ),
        (
            "scripts-run",
            "Run in new design",
            FileCommand::RunScript(generation),
            rect(x + 12. + (third + gap) * 2., y + h - 40., third, 28.),
            busy || !can_run,
        ),
    ] {
        let mut control = InterfaceControl::button("document/scripts", label);
        control.disabled = disabled;
        widgets.button(
            world,
            camera,
            key,
            control,
            Some(label),
            NativeCommand::File(command),
            bounds,
            None,
            61,
        )?;
    }
    Ok(())
}
