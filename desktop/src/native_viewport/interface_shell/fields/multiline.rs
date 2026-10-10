//! Opt-in multiline behavior for the existing native text adapter. The NC
//! source control marks its real EditableText entity; all other fields retain
//! their current submit/focus behavior. This is not a separate text editor.
use super::*;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};

#[derive(Component)]
pub(crate) struct NativeMultiline;

/// Call after constructing the NC source field. Its explicit Node dimensions
/// define the viewport; Bevy/Parley owns both scrolling axes and caret reveal.
pub(crate) fn enable(world: &mut World, entity: Entity) -> Result<(), String> {
    if world.get::<NativeTextField>(entity).is_none() {
        return Err("Multiline source requires a native text field".into());
    }
    let editor = world
        .get::<EditableText>(entity)
        .ok_or("Native text editor was removed")?;
    if !editor.allow_newlines || editor.visible_lines.is_some() {
        let mut editor = world.get_mut::<EditableText>(entity).unwrap();
        editor.allow_newlines = true;
        editor.visible_lines = None;
    }
    if world.get::<NativeMultiline>(entity).is_none() {
        world
            .entity_mut(entity)
            .insert((NativeMultiline, TextLayout::no_wrap()));
    }
    let control = world
        .get::<InterfaceControl>(entity)
        .ok_or("Native text control was removed")?;
    if control.role != "multiline_textbox" {
        world.get_mut::<InterfaceControl>(entity).unwrap().role = "multiline_textbox".into();
    }
    Ok(())
}

/// Called only for Enter, after owner validation and the native IME guard.
/// A modified submit chord retains the existing field behavior. Both Enter
/// and Shift+Enter insert one literal newline into the same undoable buffer.
pub(super) fn enter(
    world: &mut World,
    entity: Entity,
    modifiers: Modifiers,
) -> Result<bool, String> {
    if world.get::<NativeMultiline>(entity).is_none()
        || modifiers.ctrl
        || modifiers.meta
        || modifiers.alt
        || modifiers.alt_graph
    {
        return Ok(false);
    }
    if world
        .get::<EditableText>(entity)
        .is_some_and(EditableText::is_composing)
    {
        return Ok(true);
    }
    apply_edit(world, entity, TextEdit::Insert("\n".into()))?;
    Ok(true)
}

/// Consume a wheel only over the focused multiline field. TextEdit scrolls
/// the existing viewport, so pointer selection and IME use the same offsets.
pub(super) fn wheel(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    entity: Entity,
    wheel: &MouseWheel,
    cursor: Option<Vec2>,
    modifiers: Modifiers,
) -> Result<bool, String> {
    if world.get::<NativeMultiline>(entity).is_none()
        || cursor.and_then(|p| handle.hit_key(p.as_dvec2().to_array()))
            != Some(ControlKey(entity.to_bits()))
    {
        return Ok(false);
    }
    let mut x = wheel.x;
    let mut y = wheel.y;
    if !x.is_finite() || !y.is_finite() {
        return Ok(true);
    }
    if modifiers.shift && x == 0. {
        x = y;
        y = 0.;
    }
    match wheel.unit {
        MouseScrollUnit::Pixel => {
            apply_edit(world, entity, TextEdit::ScrollBy(Vec2::new(-x, -y)))?;
        }
        MouseScrollUnit::Line => {
            if x != 0. {
                let line = world
                    .get::<EditableText>(entity)
                    .ok_or("Native text editor was removed")?
                    .editor
                    .ime_cursor_area()
                    .height() as f32;
                apply_edit(
                    world,
                    entity,
                    TextEdit::ScrollBy(Vec2::new(-x * line.max(1.), 0.)),
                )?;
            }
            if y != 0. {
                apply_edit(world, entity, TextEdit::ScrollByLines(-y))?;
            }
        }
    }
    handle.invalidate_presentation();
    Ok(true)
}

#[cfg(test)]
#[path = "multiline/tests.rs"]
mod tests;
