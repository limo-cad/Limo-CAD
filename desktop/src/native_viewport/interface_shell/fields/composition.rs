//! A provisional IME edit can replace an existing selection. Keep a single
//! checkpoint of Bevy's editor so cancellation and draft Undo retain that text.
use super::*;

pub(super) fn prepare(world: &mut World, entity: Entity, edit: &TextEdit) -> Result<(), String> {
    if matches!(edit, TextEdit::ImeSetCompose { value, .. } if !value.is_empty()) {
        if world
            .get::<NativeTextField>(entity)
            .is_some_and(|field| field.composition.is_none())
        {
            flush_edits(world)?;
            let editor = world
                .get::<EditableText>(entity)
                .ok_or("Native text editor was removed")?;
            if !editor.is_composing() {
                let checkpoint = Box::new(editor.clone());
                world
                    .get_mut::<NativeTextField>(entity)
                    .unwrap()
                    .composition = Some(checkpoint);
            }
        }
    } else {
        cancel(world, entity)?;
    }
    Ok(())
}

pub(super) fn cancel(world: &mut World, entity: Entity) -> Result<(), String> {
    let Some(checkpoint) = world
        .get_mut::<NativeTextField>(entity)
        .and_then(|mut field| field.composition.take())
    else {
        return Ok(());
    };
    let alignment = world.get::<TextLayout>(entity).map(|layout| layout.justify);
    let mut text = world
        .get_mut::<EditableText>(entity)
        .ok_or("Native text editor was removed")?;
    let mut restored = checkpoint.editor;
    *restored.edit_styles() = text.editor.get_styles().clone();
    restored.set_scale(text.editor.get_scale());
    restored.set_width(Some(text.viewport.size.x));
    if let Some(alignment) = alignment {
        restored.set_alignment(alignment.into());
    }
    text.editor = restored;

    invalidate_text(world, entity);
    Ok(())
}
