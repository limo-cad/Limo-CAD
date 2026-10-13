//! Graceful window exit must preserve source drafts as well as CAD documents.
use super::*;
use bevy::text::EditableText;

pub(crate) fn guard_exit(world: &mut World) -> Result<(), String> {
    let Some(files) = world.get_resource::<Files>() else {
        return Ok(());
    };
    let state = &files.script;
    let native = state
        .source_entity
        .and_then(|entity| world.get::<EditableText>(entity));
    let error = if state.loading()
        || matches!(
            files.picker.as_ref().map(|picker| &picker.kind),
            Some(PickerKind::Script | PickerKind::ScriptSave(_))
        ) {
        Some("Finish the script file operation before closing the application.")
    } else if native.is_some_and(EditableText::is_composing) {
        Some("Finish or cancel script text composition before closing the application.")
    } else if state.dirty()
        || native.is_some_and(|editor| editor.value().to_string() != state.baseline)
        || editor::limit_error(world).is_some()
    {
        Some("Unsaved script source: Save script as or Discard edits, then close the application again.")
    } else if state.library.pending().is_some() {
        Some("Finish or cancel opening the queued example before closing the application.")
    } else {
        None
    };
    let Some(error) = error else { return Ok(()) };
    let files = &mut world.resource_mut::<Files>();
    files.scripts = true;
    files.settings = false;
    files.menu = false;
    files.script.library.open = false;
    files.script.editor_open = files.script.loaded.is_some();
    files.script.status = Some(error.into());
    Err(error.into())
}

#[cfg(test)]
#[path = "exit/tests.rs"]
mod tests;
