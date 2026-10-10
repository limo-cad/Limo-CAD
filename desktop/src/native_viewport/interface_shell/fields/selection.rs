//! Source navigation selects the existing editor. No buffer replacement, input
//! synthesis, local Undo reset, or independent text/layout state is involved.
use super::*;
use bevy::text::{FontCx, LayoutCx, TextLineYBounds};
use std::ops::Range;

pub(crate) fn select_reveal(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    entity: Entity,
    expected: &str,
    range: Range<usize>,
) -> Result<(), String> {
    let action = handle.resolve_retained(ControlKey(entity.to_bits()))?;
    validate_editor(world, handle, &action)?;
    flush_edits(world)?;
    let editor = world
        .get::<EditableText>(entity)
        .ok_or("Native text editor was removed")?;
    if editor.is_composing() || editor.value() != expected {
        return Err("The source changed before chapter navigation could run".into());
    }
    let boundary = |offset: usize| {
        let mut offset = offset.min(expected.len());
        while !expected.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    };
    let start = boundary(range.start);
    let end = boundary(range.end).max(start);
    world.resource_scope(|world, mut fonts: Mut<FontCx>| {
        world.resource_scope(|world, mut layout: Mut<LayoutCx>| {
            let mut text = world.get_mut::<EditableText>(entity).unwrap();
            let EditableText {
                editor,
                viewport,
                cursor_margin,
                ..
            } = &mut *text;
            let mut driver = editor.driver(&mut fonts.context, &mut layout.0);
            driver.select_byte_range(end, start);
            if let Some(caret) = driver.editor.cursor_geometry(1.) {
                let bounds = Rect::new(
                    caret.x0 as f32,
                    caret.y0 as f32,
                    caret.x1 as f32,
                    caret.y1 as f32,
                );
                let layout = driver.layout();
                viewport.reveal_caret(
                    bounds,
                    Vec2::new(layout.full_width(), layout.height()),
                    *cursor_margin,
                    layout.lines().map(|line| TextLineYBounds::from_line(&line)),
                );
            }
        });
    });
    handle.assistive_action(&action, false)?;
    after_window_input(world, handle)?;
    invalidate_text(world, entity);
    handle.invalidate_presentation();
    Ok(())
}
