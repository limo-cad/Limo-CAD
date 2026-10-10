//! Native field adapters. Parley/Bevy owns Unicode editing and text layout;
//! committed values take the same owned SetValue path as MCP.

use bevy::{
    input::{
        keyboard::{Key, KeyCode, KeyboardInput},
        ButtonState,
    },
    prelude::*,
    text::{EditableText, FontWeight, PreeditCursor, TextCursorStyle, TextEdit},
    ui::{ComputedUiRenderTargetInfo, UiGlobalTransform, UiScale, UiSystems},
    window::{Ime, PrimaryWindow, WindowEvent},
};
use limo_cad_interface::{ControlInput, ControlKey, DocumentContext, Field};
use std::collections::VecDeque;

use super::{
    InterfaceControl, InterfaceLayout, InterfaceTextRevision, NativeInterfaceAction,
    NativeInterfaceHandle,
};
use crate::native_viewport::{
    ui::{ViewportUiAssets, ViewportUiTheme},
    winit_host::Modifiers,
};

mod composition;
mod ime_popup;
pub(crate) mod limits;
pub(crate) mod multiline;
mod selection;
pub(crate) use selection::select_reveal;

type EditableFieldQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static InterfaceControl,
        &'static mut NativeTextField,
        &'static mut EditableText,
        &'static mut InterfaceTextRevision,
        &'static mut Node,
        &'static mut BorderColor,
        Option<&'static mut Outline>,
        Option<&'static DrawingDimension>,
    ),
>;

type ImeFieldQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static EditableText,
        &'static ComputedNode,
        &'static UiGlobalTransform,
    ),
    (With<NativeTextField>, With<ComputedUiRenderTargetInfo>),
>;

/// Repaint a retained editor without replacing its buffer, preedit, caret,
/// local Undo history or generational input binding.
pub(super) fn refresh_theme(world: &mut World, theme: ViewportUiTheme) {
    let mut fields = world.query::<(
        &mut NativeTextField,
        &mut TextColor,
        &mut TextCursorStyle,
        &mut BackgroundColor,
    )>();
    for (mut field, mut ink, mut cursor, mut fill) in fields.iter_mut(world) {
        field.theme = theme;
        ink.0 = theme.ink;
        fill.0 = theme.panel;
        cursor.color = theme.ink;
        cursor.selection_color = theme.accent.with_alpha(0.65);
        cursor.unfocused_selection_color = theme.accent.with_alpha(0.3);
    }
}

#[derive(Component)]
#[require(EditableText, Node, bevy::ui::TextNodeFlags, bevy::ui::ContentSize)]
pub(crate) struct NativeTextField {
    baseline: String,
    queued: Option<String>,
    undo: VecDeque<String>,
    redo: VecDeque<String>,
    composition: Option<Box<EditableText>>,
    binding: u64,
    theme: ViewportUiTheme,
}

#[derive(Resource, Default)]
struct EditorSession {
    active: Option<NativeInterfaceAction>,
}

/// Dimensions that replace their preview value on keyboard entry and keep
/// Tab within the current construction gesture.
#[derive(Component)]
pub(crate) struct DrawingDimension {
    pub generation: u64,
    pub index: usize,
}

/// Draft values with a live preview or a second editor publish each completed
/// edit through the same owner-checked SetValue reducer. IME preedit stays local.
#[derive(Component)]
pub(crate) struct LiveValue;

/// Presentation only: the real editor, owned value and selection stay exact.
#[derive(Component)]
struct CompactNumberCaption(Entity);

pub(crate) fn compact_number_caption(
    world: &mut World,
    entity: Entity,
    theme: ViewportUiTheme,
    assets: &ViewportUiAssets,
) {
    if world.get::<NativeTextField>(entity).is_none()
        || world.get::<CompactNumberCaption>(entity).is_some()
    {
        return;
    }
    let caption = world
        .spawn((
            Text::new(""),
            TextLayout::no_wrap(),
            theme.text(assets, 13., FontWeight::NORMAL),
            TextColor(theme.ink),
            Node {
                position_type: PositionType::Absolute,
                left: px(7.),
                right: px(22.),
                top: px(3.),
                height: px(20.),
                overflow: Overflow::clip(),
                display: Display::None,
                ..default()
            },
            bevy::picking::Pickable::IGNORE,
        ))
        .id();
    world
        .entity_mut(entity)
        .add_child(caption)
        .insert(CompactNumberCaption(caption));
}

fn compact_numeric_text(raw: &str) -> Option<String> {
    // Expressions and whitespace remain literal. Nine significant digits are
    // a read-only presentation; focus always reveals the unchanged raw number.
    if !raw
        .bytes()
        .all(|c| c.is_ascii_digit() || b"-+.eE".contains(&c))
    {
        return None;
    }
    let value: f64 = raw.parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    if value == 0.
        && raw
            .split(['e', 'E'])
            .next()?
            .bytes()
            .any(|c| (b'1'..=b'9').contains(&c))
    {
        // A literal below f64's range must never be presented as exact zero.
        return None;
    }
    let scientific = format!("{value:.8e}");
    let (mantissa, exponent) = scientific.split_once('e')?;
    let exponent: i32 = exponent.parse().ok()?;
    let compact = if value != 0. && !(-3..=8).contains(&exponent) {
        format!(
            "{}e{exponent}",
            mantissa.trim_end_matches('0').trim_end_matches('.')
        )
    } else {
        let decimals = (8 - exponent).max(0) as usize;
        let fixed = format!("{value:.decimals$}");
        if fixed.contains('.') {
            fixed.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            fixed
        }
    };
    (compact.len() < raw.len()).then_some(compact)
}

fn update_compact_numbers(world: &mut World) {
    let focused = world.resource::<NativeInterfaceHandle>().focused_key();
    let mut query = world.query::<(
        Entity,
        &CompactNumberCaption,
        &NativeTextField,
        &EditableText,
    )>();
    let updates: Vec<_> = query
        .iter(world)
        .map(|(entity, caption, field, editor)| {
            let compact = (focused != Some(ControlKey(entity.to_bits()))
                && field.queued.is_none()
                && field.composition.is_none()
                && !editor.is_composing()
                && editor.value() == field.baseline.as_str())
            .then(|| compact_numeric_text(field.baseline.as_str()))
            .flatten();
            (entity, caption.0, field.theme, compact)
        })
        .collect();
    for (entity, caption, theme, compact) in updates {
        let color = if compact.is_some() {
            Color::NONE
        } else {
            theme.ink
        };
        if let Some(mut ink) = world.get_mut::<TextColor>(entity) {
            if ink.0 != color {
                ink.0 = color;
            }
        }
        if let Some(mut node) = world.get_mut::<Node>(caption) {
            let display = if compact.is_some() {
                Display::Flex
            } else {
                Display::None
            };
            if node.display != display {
                node.display = display;
            }
        }
        if let Some(value) = compact {
            if let Some(mut text) = world.get_mut::<Text>(caption) {
                if text.0 != value {
                    text.0 = value;
                }
            }
            if let Some(mut ink) = world.get_mut::<TextColor>(caption) {
                if ink.0 != theme.ink {
                    ink.0 = theme.ink;
                }
            }
        }
    }
}

#[derive(Resource, Default)]
struct RequestedFocus(Option<FocusRequest>);

struct FocusRequest {
    entity: Entity,
    binding: u64,
    owner: DocumentContext,
    modal_scope: Option<String>,
}

pub(crate) fn request_focus(world: &mut World, entity: Entity, owner: &DocumentContext) {
    let control = world.get::<InterfaceControl>(entity).unwrap();
    let request = FocusRequest {
        entity,
        binding: control.binding,
        owner: owner.clone(),
        modal_scope: control.modal_scope.clone(),
    };
    world.insert_resource(RequestedFocus(Some(request)));
}

/// A moving preview must never replace a number or expression being typed,
/// including the interval between submitting it and the reducer accepting it.
pub(crate) fn has_uncommitted_edit(world: &World, entity: Entity) -> bool {
    world.get::<NativeTextField>(entity).is_some_and(|field| {
        field.queued.is_some()
            || world.get::<EditableText>(entity).is_some_and(|editor| {
                editor.is_composing() || editor.value() != field.baseline.as_str()
            })
    })
}

fn apply_requested_focus(world: &mut World) {
    let Some(request) = world.resource_mut::<RequestedFocus>().0.take() else {
        return;
    };
    let FocusRequest {
        entity,
        binding,
        ref owner,
        ref modal_scope,
    } = request;
    let handle = world.resource::<NativeInterfaceHandle>().clone();
    if world.get::<InterfaceControl>(entity).is_none_or(|control| {
        control.binding != binding
            || control.modal_scope != *modal_scope
            || !control.visible
            || control.disabled
    }) || handle.frame().is_none_or(|frame| {
        frame.context != *owner || frame.modal_stack.last() != modal_scope.as_ref()
    }) {
        return;
    }
    // Newly spawned editors can need another text/layout pass before their
    // hit areas are published. Retain the original owner and binding while
    // waiting; never resolve this request against a replacement gesture.
    let action = handle
        .resolve_retained(ControlKey(entity.to_bits()))
        .ok()
        .filter(|action| action.context == *owner && action.control.binding() == binding);
    let Some(action) = action else {
        world.resource_mut::<RequestedFocus>().0 = Some(request);
        handle.request_redraw();
        return;
    };
    if validate_editor(world, &handle, &action).is_err()
        || handle.prepare_activation(&action).is_err()
    {
        return;
    }
    if let Err(error) = after_window_input(world, &handle)
        .and_then(|()| apply_edit(world, entity, TextEdit::SelectAll))
    {
        eprintln!("Native field focus failed: {error}");
        return;
    }
    // Programmatic focus invites a replacement value, just as the reference
    // number editors do. Pointer focus still places the caret at the click.
    handle.invalidate_presentation();
    if let Some(mut focus) = world.get_resource_mut::<bevy::input_focus::InputFocus>() {
        focus.set(entity, bevy::input_focus::FocusCause::Navigated);
    }
}

pub(crate) fn install(app: &mut App) {
    if app
        .world()
        .get_resource::<NativeInterfaceHandle>()
        .is_some_and(super::ime_diagnostics::enabled)
    {
        app.add_systems(First, super::ime_diagnostics::observe_configuration);
    }
    app.init_resource::<EditorSession>()
        .init_resource::<RequestedFocus>()
        .init_resource::<ime_popup::ImeCandidateWindow>()
        .add_systems(Update, synchronize_fields.after(super::InterfaceReduction))
        .add_systems(Update, update_compact_numbers.after(synchronize_fields))
        .add_systems(
            PostUpdate,
            apply_requested_focus
                .after(InterfaceLayout)
                .before(update_ime),
        )
        .add_systems(
            PostUpdate,
            update_ime
                .after(InterfaceLayout)
                .after(UiSystems::Stack)
                .after(bevy::ui::widget::update_editable_text_layout),
        );
    if app
        .world()
        .contains_resource::<bevy::app::MainScheduleOrder>()
    {
        app.init_schedule(ime_popup::PlaceImeCandidate);
        {
            let mut order = app
                .world_mut()
                .resource_mut::<bevy::app::MainScheduleOrder>();
            if !order
                .labels
                .iter()
                .any(|current| (**current).eq(&ime_popup::PlaceImeCandidate))
            {
                order.insert_after(Last, ime_popup::PlaceImeCandidate);
            }
        }
        app.add_systems(ime_popup::PlaceImeCandidate, ime_popup::place_os_candidate);
    }
}

/// Root binds the returned real widget to its typed form field, exactly as it
/// binds buttons. Optional unfocused captions never replace the editable value.
pub(crate) fn spawn_text_field(
    commands: &mut Commands,
    camera: Entity,
    node: Node,
    mut control: InterfaceControl,
    theme: ViewportUiTheme,
    assets: &ViewportUiAssets,
) -> Result<Entity, String> {
    let Field::Text { value, .. } = &control.field else {
        return Err("Native text widget needs a text field".into());
    };
    let value = value.clone();
    control.text_editing = true;
    control.role = "textbox".into();
    Ok(commands
        .spawn((
            Name::new(format!("Native text: {}", control.label)),
            NativeTextField {
                baseline: value.clone(),
                queued: None,
                undo: VecDeque::new(),
                redo: VecDeque::new(),
                composition: None,
                binding: control.binding,
                theme,
            },
            EditableText::new(value),
            TextLayout::no_wrap(),
            InterfaceTextRevision::default(),
            control,
            node,
            UiTargetCamera(camera),
            theme.text(assets, 13.0, FontWeight::NORMAL),
            TextColor(theme.ink),
            TextCursorStyle {
                color: theme.ink,
                selection_color: theme.accent.with_alpha(0.65),
                unfocused_selection_color: theme.accent.with_alpha(0.3),
                ..default()
            },
            BackgroundColor(theme.panel),
            BorderColor::all(theme.edge),
            Outline::new(px(2.), px(1.), Color::NONE),
            ZIndex(31),
        ))
        .id())
}

fn active_entity(action: &NativeInterfaceAction) -> Entity {
    Entity::from_bits(action.control.key.0)
}

fn validate_editor(
    world: &World,
    handle: &NativeInterfaceHandle,
    action: &NativeInterfaceAction,
) -> Result<(), String> {
    handle.validate_action(action)?;
    let control = world
        .get::<InterfaceControl>(active_entity(action))
        .ok_or("Native text field was removed")?;
    if control.binding != action.control.binding() || !control.visible || control.disabled {
        return Err("Native text field changed before input could run".into());
    }
    Ok(())
}

fn flush_edits(world: &mut World) -> Result<(), String> {
    world
        .run_system_cached(bevy::text::apply_text_edits)
        .map_err(|error| format!("Native text edit failed: {error}"))
}

fn invalidate_text(world: &mut World, entity: Entity) {
    if let Some(mut revision) = world.get_mut::<InterfaceTextRevision>(entity) {
        revision.0 = revision.0.wrapping_add(1);
    }
}

/// Flush before changing focus, including background clicks and Tab. This
/// captures the original binding before root reduces the value; stale owners
/// can never be repaired by resolving their field in a replacement document.
fn commit_active(
    world: &mut World,
    handle: &NativeInterfaceHandle,
) -> Result<Option<NativeInterfaceAction>, String> {
    let Some(action) = world.resource::<EditorSession>().active.clone() else {
        return Ok(None);
    };
    if world
        .get::<NativeTextField>(active_entity(&action))
        .is_none_or(|field| field.binding != action.control.binding())
        || handle
            .frame()
            .is_none_or(|frame| frame.context != action.context)
    {
        world.resource_mut::<EditorSession>().active = None;
        return Ok(None);
    }
    if let Err(error) = validate_editor(world, handle, &action) {
        world.resource_mut::<EditorSession>().active = None;
        return Err(error);
    }
    flush_edits(world)?;
    let entity = active_entity(&action);
    composition::cancel(world, entity)?;
    if limits::error(world, entity).is_some() {
        return Ok(None);
    }
    let value = world
        .get::<EditableText>(entity)
        .ok_or("Native text editor was removed")?
        .value()
        .to_string();
    if world
        .get::<NativeTextField>(entity)
        .is_some_and(|field| field.baseline == value || field.queued.as_ref() == Some(&value))
    {
        return Ok(None);
    }
    let committed = handle.resolve_input(
        action.control.key,
        ControlInput::SetValue(value.clone()),
        &action.context,
    )?;
    world
        .get_mut::<NativeTextField>(entity)
        .ok_or("Native text editor was removed")?
        .queued = Some(value);
    Ok(Some(committed))
}

/// Only accepted SetValue commits advance the editor baseline. A rejected
/// draft stays dirty, so the next Apply cannot use the old accepted value.
pub(crate) fn acknowledge_control_input(
    world: &mut World,
    action: &NativeInterfaceAction,
    accepted: bool,
) {
    let ControlInput::SetValue(value) = &action.control.input else {
        return;
    };
    let entity = active_entity(action);
    if let Some(mut field) = world.get_mut::<NativeTextField>(entity) {
        if field.binding != action.control.binding() {
            return;
        }
        if field.queued.as_ref() == Some(value) {
            field.queued = None;
        }
        if accepted {
            field.baseline.clone_from(value);
            field.composition = None;
        }

        if accepted {
            if let Some(mut editor) = world.get_mut::<EditableText>(entity) {
                if editor.is_composing() || editor.value().to_string() != *value {
                    editor.editor.set_text(value);
                    editor.pending_edits.clear();
                    editor.pending_paste = None;
                    editor.queue_edit(TextEdit::clear_ime_compose());
                    editor.queue_edit(TextEdit::TextEnd(false));
                }
            }
            invalidate_text(world, entity);
        }
    }
}

#[cfg(test)]
mod tests;

/// The shared reducer applies these preceding commits, checks their results,
/// then revalidates and applies the original target. A failed blur commit must
/// never allow an Apply button to run against the previous value.
pub(crate) fn prepare_control_input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    action: &NativeInterfaceAction,
) -> Result<Vec<NativeInterfaceAction>, String> {
    handle.validate_action(action)?;
    if !world.contains_resource::<EditorSession>() {
        return Ok(Vec::new());
    }
    if world
        .resource::<EditorSession>()
        .active
        .as_ref()
        .is_some_and(|active| active.control.key == action.control.key)
    {
        return Ok(Vec::new());
    }
    Ok(commit_active(world, handle)?.into_iter().collect())
}

/// A queued text commit belongs to the field that just lost focus. Applying
/// its value must not undo the focus transition already made by Tab or blur.
pub(crate) fn prepare_activation(
    world: &World,
    handle: &NativeInterfaceHandle,
    action: &NativeInterfaceAction,
) -> Result<(), String> {
    let queued = match &action.control.input {
        ControlInput::SetValue(value) => world
            .get::<NativeTextField>(active_entity(action))
            .is_some_and(|field| {
                field.binding == action.control.binding() && field.queued.as_ref() == Some(value)
            }),
        _ => false,
    };
    if queued {
        handle.validate_action(action)
    } else {
        handle.prepare_activation(action)
    }
}

/// Widget keys use the same edit operations regardless of their transport.
/// Escape belongs to the owning form; arrows merely update caret/selection.
pub(crate) fn adapt_control_input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    action: &NativeInterfaceAction,
) -> Result<Option<NativeInterfaceAction>, String> {
    let entity = active_entity(action);
    if world.get::<NativeTextField>(entity).is_some()
        && matches!(
            action.control.input,
            ControlInput::Click | ControlInput::DoubleClick
        )
    {
        validate_editor(world, handle, action)?;
        return Ok(None);
    }
    let ControlInput::Key(chord) = &action.control.input else {
        return Ok(Some(action.clone()));
    };
    if world.get::<NativeTextField>(entity).is_none() || chord.key == "Escape" {
        return Ok(Some(action.clone()));
    }
    validate_editor(world, handle, action)?;
    let modifiers = Modifiers {
        ctrl: chord.ctrl,
        meta: chord.meta,
        alt: chord.alt,
        shift: chord.shift,
        alt_graph: false,
    };
    if chord.key == "Enter" {
        if multiline::enter(world, entity, modifiers)? {
            handle.invalidate_presentation();
            return Ok(None);
        }
        let commit = commit_active(world, handle)?;
        if submits_on_enter(world, entity) {
            if commit.is_some() {
                handle.enqueue_action(action.clone())?;
            } else {
                return Ok(Some(action.clone()));
            }
        }
        return Ok(commit);
    }
    let logical_key = match chord.key.as_str() {
        "ArrowLeft" => Key::ArrowLeft,
        "ArrowRight" => Key::ArrowRight,
        "ArrowUp" => Key::ArrowUp,
        "ArrowDown" => Key::ArrowDown,
        "Home" => Key::Home,
        "End" => Key::End,
        "Backspace" => Key::Backspace,
        "Delete" => Key::Delete,
        _ => return Ok(None),
    };
    if let Some(edit) = logical_edit(&logical_key, None, modifiers) {
        apply_edit(world, entity, edit)?;
    }
    handle.invalidate_presentation();
    commit_active(world, handle)
}

fn submits_on_enter(world: &World, entity: Entity) -> bool {
    world
        .get::<InterfaceControl>(entity)
        .is_some_and(|control| {
            control
                .owned_keys
                .contains(&limo_cad_interface::KeyChord::plain("Enter"))
        })
}

fn trim_history(field: &mut NativeTextField) {
    const MAX_HISTORY_BYTES: usize = 8 * 1024 * 1024;
    while field.undo.len() + field.redo.len() > 128
        || field
            .undo
            .iter()
            .chain(&field.redo)
            .map(String::len)
            .sum::<usize>()
            > MAX_HISTORY_BYTES
    {
        if field.undo.pop_front().is_none() {
            field.redo.pop_front();
        }
    }
}

fn apply_edit(world: &mut World, entity: Entity, edit: TextEdit) -> Result<(), String> {
    let edit = match edit {
        TextEdit::ImeCommit { ref value } if value.is_empty() => TextEdit::clear_ime_compose(),
        edit => edit,
    };
    let read_only = matches!(
        world
            .get::<InterfaceControl>(entity)
            .map(|control| &control.field),
        Some(Field::Text {
            read_only: true,
            ..
        })
    );
    if read_only && edit.is_destructive() {
        return Ok(());
    }
    let edit = limits::before_edit(world, entity, edit)?;
    composition::prepare(world, entity, &edit)?;
    let records_history = edit.is_destructive() && !matches!(edit, TextEdit::ImeSetCompose { .. });
    let before = records_history.then(|| {
        world
            .get::<EditableText>(entity)
            .unwrap()
            .value()
            .to_string()
    });
    world
        .get_mut::<EditableText>(entity)
        .ok_or("Native text editor was removed")?
        .queue_edit(edit);
    flush_edits(world)?;
    invalidate_text(world, entity);
    if let Some(before) = before {
        if world
            .get::<EditableText>(entity)
            .unwrap()
            .value()
            .to_string()
            != before
        {
            limits::clear(world, entity);
            let mut field = world
                .get_mut::<NativeTextField>(entity)
                .ok_or("Native text field was removed")?;
            field.undo.push_back(before);
            field.redo.clear();
            trim_history(&mut field);
        }
    }
    Ok(())
}

fn history_edit(world: &mut World, entity: Entity, redo: bool) -> Result<(), String> {
    if matches!(
        world
            .get::<InterfaceControl>(entity)
            .map(|control| &control.field),
        Some(Field::Text {
            read_only: true,
            ..
        })
    ) {
        return Ok(());
    }
    flush_edits(world)?;
    composition::cancel(world, entity)?;
    let current = world
        .get::<EditableText>(entity)
        .ok_or("Native editor was removed")?
        .value()
        .to_string();
    let mut field = world
        .get_mut::<NativeTextField>(entity)
        .ok_or("Native field was removed")?;
    let Some(value) = (if redo {
        &mut field.redo
    } else {
        &mut field.undo
    })
    .pop_back() else {
        return Ok(());
    };
    let changed = value != current;
    if redo {
        field.undo.push_back(current);
    } else {
        field.redo.push_back(current);
    }
    trim_history(&mut field);

    let mut editor = world
        .get_mut::<EditableText>(entity)
        .ok_or("Native editor was removed")?;
    editor.editor.set_text(&value);
    editor.pending_edits.clear();
    editor.queue_edit(TextEdit::TextEnd(false));

    flush_edits(world)?;
    invalidate_text(world, entity);
    if changed {
        limits::clear(world, entity);
    }
    Ok(())
}

/// Called for every original OS event before general UI/model routing. This
/// preserves typing → Tab → typing order even within one event-loop update.
pub(crate) fn before_window_input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    event: &WindowEvent,
    cursor: Option<Vec2>,
    modifiers: Modifiers,
) -> Result<bool, String> {
    if !world.contains_resource::<EditorSession>() {
        return Ok(false);
    }
    let Some(action) = world.resource::<EditorSession>().active.clone() else {
        return Ok(false);
    };
    if validate_editor(world, handle, &action).is_err() {
        world.resource_mut::<EditorSession>().active = None;
        return Ok(matches!(event, WindowEvent::Ime(_)));
    }
    super::ime_diagnostics::received_keyboard(world, handle, &action, event);
    let entity = active_entity(&action);
    let composing = world
        .get::<EditableText>(entity)
        .is_some_and(EditableText::is_composing);
    let edit = match event {
        WindowEvent::Ime(Ime::Preedit { value, cursor, .. }) => Some(TextEdit::ImeSetCompose {
            value: value.as_str().into(),
            cursor: cursor.map(|(anchor, focus)| PreeditCursor { anchor, focus }),
        }),
        WindowEvent::Ime(Ime::Commit { value, .. }) => Some(TextEdit::ImeCommit {
            value: value.as_str().into(),
        }),
        WindowEvent::Ime(Ime::Enabled { .. } | Ime::Disabled { .. }) => {
            Some(TextEdit::clear_ime_compose())
        }
        WindowEvent::KeyboardInput(input) if input.state == ButtonState::Pressed => {
            if composing {
                return Ok(true);
            }
            let command = command_modifier(modifiers, cfg!(target_os = "macos"));
            if command {
                if let Key::Character(key) = shortcut_key(input) {
                    if key.eq_ignore_ascii_case("z")
                        || (!cfg!(target_os = "macos") && key.eq_ignore_ascii_case("y"))
                    {
                        let redo = modifiers.shift || key.eq_ignore_ascii_case("y");
                        if world.get::<DrawingDimension>(entity).is_some()
                            && !has_uncommitted_edit(world, entity)
                            && !(redo
                                && world
                                    .get::<NativeTextField>(entity)
                                    .is_some_and(|field| !field.redo.is_empty()))
                        {
                            world.resource_mut::<EditorSession>().active = None;
                            handle.blur();
                            return Ok(false);
                        }
                        history_edit(world, entity, redo)?;
                        publish_live_value(world, handle, entity)?;
                        handle.invalidate_presentation();
                        return Ok(true);
                    }
                }
            }
            if input.logical_key == Key::Enter && multiline::enter(world, entity, modifiers)? {
                handle.invalidate_presentation();
                return Ok(true);
            }
            if input.logical_key == Key::Tab || input.logical_key == Key::Enter {
                if let Some(commit) = commit_active(world, handle)? {
                    handle.enqueue_action(commit)?;
                }
                if input.logical_key == Key::Tab
                    && focus_drawing_dimension(world, handle, entity, modifiers.shift)?
                {
                    return Ok(true);
                }
                if input.logical_key == Key::Tab
                    && crate::native_editor::panel::focus_adjacent_form_field(
                        world,
                        handle,
                        entity,
                        modifiers.shift,
                    )?
                {
                    return Ok(true);
                }
                if input.logical_key == Key::Enter && submits_on_enter(world, entity) {
                    handle.key(limo_cad_interface::KeyChord::plain("Enter"))?;
                }
                return Ok(input.logical_key == Key::Enter);
            }
            keyboard_edit(input, modifiers)
        }
        WindowEvent::MouseWheel(wheel) => {
            return multiline::wheel(world, handle, entity, wheel, cursor, modifiers);
        }
        WindowEvent::MouseButtonInput(input) if input.state == ButtonState::Pressed => {
            let target = cursor.and_then(|cursor| handle.hit_key(cursor.as_dvec2().to_array()));
            if target != Some(action.control.key) {
                if let Some(commit) = commit_active(world, handle)? {
                    handle.enqueue_action(commit)?;
                }
                world.resource_mut::<EditorSession>().active = None;
                handle.blur();
            }
            None
        }
        WindowEvent::WindowFocused(event) if !event.focused => {
            if let Some(commit) = commit_active(world, handle)? {
                handle.enqueue_action(commit)?;
            }
            world.resource_mut::<EditorSession>().active = None;
            None
        }
        _ => None,
    };
    if let Some(edit) = edit {
        apply_edit(world, entity, edit)?;
        publish_live_value(world, handle, entity)?;
        super::ime_diagnostics::received(world, handle, &action, event);
        handle.invalidate_presentation();
        return Ok(true);
    }
    Ok(false)
}

fn publish_live_value(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    entity: Entity,
) -> Result<(), String> {
    if world.get::<LiveValue>(entity).is_some()
        && world
            .get::<EditableText>(entity)
            .is_some_and(|editor| !editor.is_composing())
    {
        if let Some(commit) = commit_active(world, handle)? {
            handle.enqueue_action(commit)?;
        }
    }
    Ok(())
}

pub(crate) fn after_window_input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
) -> Result<(), String> {
    synchronize_editor_focus(world, handle, true)
}

/// Returning to a window resumes its editor without reselecting a dimension.
pub(crate) fn after_window_focus(
    world: &mut World,
    handle: &NativeInterfaceHandle,
) -> Result<(), String> {
    synchronize_editor_focus(world, handle, false)
}

fn synchronize_editor_focus(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    select_dimension: bool,
) -> Result<(), String> {
    if !world.contains_resource::<EditorSession>() {
        return Ok(());
    }
    let next = handle.focused_key().filter(|key| {
        world
            .get::<NativeTextField>(Entity::from_bits(key.0))
            .is_some()
    });
    let current = world
        .resource::<EditorSession>()
        .active
        .as_ref()
        .map(|active| active.control.key);
    if current == next {
        return Ok(());
    }
    if let Some(commit) = commit_active(world, handle)? {
        handle.enqueue_action(commit)?;
    }
    let action = next.map(|key| handle.resolve_retained(key)).transpose()?;
    if let Some(entity) = next.map(|key| Entity::from_bits(key.0)) {
        if select_dimension && world.get::<DrawingDimension>(entity).is_some() {
            apply_edit(world, entity, TextEdit::SelectAll)?;
            handle.invalidate_presentation();
        }
    }
    world.resource_mut::<EditorSession>().active = action;
    Ok(())
}

fn focus_drawing_dimension(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    current: Entity,
    backwards: bool,
) -> Result<bool, String> {
    let Some(group) = world.get::<DrawingDimension>(current) else {
        return Ok(false);
    };
    let generation = group.generation;
    let owner = handle
        .resolve_retained(ControlKey(current.to_bits()))?
        .context;
    let mut fields = world.query::<(Entity, &DrawingDimension, &InterfaceControl)>();
    let mut candidates: Vec<_> = fields
        .iter(world)
        .filter(|(_, group, control)| {
            group.generation == generation && control.visible && !control.disabled
        })
        .filter_map(|(entity, group, _)| {
            handle
                .resolve_retained(ControlKey(entity.to_bits()))
                .ok()
                .filter(|action| {
                    action.context == owner && validate_editor(world, handle, action).is_ok()
                })
                .map(|action| (group.index, action))
        })
        .collect();
    candidates.sort_by_key(|(index, _)| *index);
    let Some(index) = candidates
        .iter()
        .position(|(_, action)| active_entity(action) == current)
    else {
        return Ok(false);
    };
    let next = if backwards {
        (index + candidates.len() - 1) % candidates.len()
    } else {
        (index + 1) % candidates.len()
    };
    handle.prepare_activation(&candidates[next].1)?;
    after_window_input(world, handle)?;
    if next == index {
        apply_edit(world, current, TextEdit::SelectAll)?;
    }
    Ok(true)
}

pub(crate) fn after_pointer_input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    event: &WindowEvent,
    cursor: Option<Vec2>,
    modifiers: Modifiers,
) -> Result<(), String> {
    let Some(action) = world
        .get_resource::<EditorSession>()
        .and_then(|state| state.active.clone())
    else {
        return Ok(());
    };
    let Some(cursor) = cursor else {
        return Ok(());
    };
    let press = matches!(event, WindowEvent::MouseButtonInput(input) if input.button == MouseButton::Left && input.state == ButtonState::Pressed);
    let drag = matches!(event, WindowEvent::CursorMoved(_)) && handle.has_capture();
    if !press && !drag {
        return Ok(());
    }
    validate_editor(world, handle, &action)?;
    let entity = active_entity(&action);
    let Some(node) = world.get::<ComputedNode>(entity) else {
        return Ok(());
    };
    let Some(transform) = world
        .get::<UiGlobalTransform>(entity)
        .and_then(UiGlobalTransform::try_inverse)
    else {
        return Ok(());
    };
    let Some(target) = world.get::<ComputedUiRenderTargetInfo>(entity) else {
        return Ok(());
    };
    let scroll = world
        .get::<EditableText>(entity)
        .map_or(Vec2::ZERO, |editor| editor.viewport.offset);
    let point = transform.transform_point2(cursor * target.scale_factor()) - node.content_box().min
        + scroll;
    let edit = if drag {
        TextEdit::ExtendSelectionToPoint(point)
    } else if modifiers.shift {
        TextEdit::ShiftClickExtension(point)
    } else {
        TextEdit::MoveToPoint(point)
    };
    world
        .get_mut::<EditableText>(entity)
        .ok_or("Native text editor was removed")?
        .queue_edit(edit);
    flush_edits(world)?;
    invalidate_text(world, entity);
    Ok(())
}

fn keyboard_edit(input: &KeyboardInput, modifiers: Modifiers) -> Option<TextEdit> {
    let key = if command_modifier(modifiers, cfg!(target_os = "macos")) {
        shortcut_key(input)
    } else {
        input.logical_key.clone()
    };
    logical_edit(&key, input.text.as_deref(), modifiers)
}

fn command_modifier(modifiers: Modifiers, mac: bool) -> bool {
    !modifiers.alt_graph && !modifiers.alt && if mac { modifiers.meta } else { modifiers.ctrl }
}

/// Preserve Latin keyboard-layout shortcuts; fall back to the physical key
/// only for non-Latin layouts, matching Bevy's native TextInput adapter.
pub(crate) fn shortcut_key(input: &KeyboardInput) -> Key {
    if matches!(&input.logical_key, Key::Character(value) if !value.is_ascii() || value.chars().all(char::is_control))
    {
        let letter = match input.key_code {
            KeyCode::KeyA => Some("a"),
            KeyCode::KeyC => Some("c"),
            KeyCode::KeyX => Some("x"),
            KeyCode::KeyV => Some("v"),
            KeyCode::KeyZ => Some("z"),
            KeyCode::KeyY => Some("y"),
            _ => None,
        };
        if let Some(letter) = letter {
            return Key::Character(letter.into());
        }
    }
    input.logical_key.clone()
}

fn logical_edit(key: &Key, text: Option<&str>, modifiers: Modifiers) -> Option<TextEdit> {
    logical_edit_for_platform(key, text, modifiers, cfg!(target_os = "macos"))
}

fn logical_edit_for_platform(
    key: &Key,
    text: Option<&str>,
    modifiers: Modifiers,
    mac: bool,
) -> Option<TextEdit> {
    let command = command_modifier(modifiers, mac);
    let word = if mac { modifiers.alt } else { modifiers.ctrl };
    let shift = modifiers.shift;
    match key {
        Key::Character(value) if command && value.eq_ignore_ascii_case("a") => {
            Some(TextEdit::SelectAll)
        }
        Key::Character(value) if command && value.eq_ignore_ascii_case("c") => Some(TextEdit::Copy),
        Key::Character(value) if command && value.eq_ignore_ascii_case("x") => Some(TextEdit::Cut),
        Key::Character(value) if command && value.eq_ignore_ascii_case("v") => {
            Some(TextEdit::Paste)
        }
        Key::Copy => Some(TextEdit::Copy),
        Key::Cut => Some(TextEdit::Cut),
        Key::Paste => Some(TextEdit::Paste),
        Key::Backspace => Some(if word {
            TextEdit::BackspaceWord
        } else {
            TextEdit::Backspace
        }),
        Key::Delete => Some(if word {
            TextEdit::DeleteWord
        } else {
            TextEdit::Delete
        }),
        Key::ArrowLeft => Some(if mac && command {
            TextEdit::HardLineStart(shift)
        } else if word {
            TextEdit::WordLeft(shift)
        } else {
            TextEdit::Left(shift)
        }),
        Key::ArrowRight => Some(if mac && command {
            TextEdit::HardLineEnd(shift)
        } else if word {
            TextEdit::WordRight(shift)
        } else {
            TextEdit::Right(shift)
        }),
        Key::ArrowUp => Some(if command {
            TextEdit::TextStart(shift)
        } else {
            TextEdit::Up(shift)
        }),
        Key::ArrowDown => Some(if command {
            TextEdit::TextEnd(shift)
        } else {
            TextEdit::Down(shift)
        }),
        Key::Home => Some(if command {
            TextEdit::TextStart(shift)
        } else {
            TextEdit::LineStart(shift)
        }),
        Key::End => Some(if command {
            TextEdit::TextEnd(shift)
        } else {
            TextEdit::LineEnd(shift)
        }),
        Key::Character(_) | Key::Unidentified(_) | Key::Space
            if modifiers.alt_graph
                || (!modifiers.ctrl && !modifiers.meta && (!modifiers.alt || mac)) =>
        {
            text.map(|value| TextEdit::Insert(value.into()))
        }
        _ => None,
    }
}

fn synchronize_fields(
    handle: Res<NativeInterfaceHandle>,
    mut focus: ResMut<bevy::input_focus::InputFocus>,
    mut fields: EditableFieldQuery,
) {
    let focused = handle.focused_key();
    let editor_focus = focused.filter(|key| fields.get(Entity::from_bits(key.0)).is_ok());
    if let Some(key) = editor_focus {
        focus.set(
            Entity::from_bits(key.0),
            bevy::input_focus::FocusCause::Navigated,
        );
    }
    for (
        entity,
        control,
        mut field,
        mut editor,
        mut revision,
        mut node,
        mut border,
        outline,
        dimension,
    ) in &mut fields
    {
        let Field::Text { value, .. } = &control.field else {
            continue;
        };
        if field.binding != control.binding || field.baseline != *value {
            editor.editor.set_text(value);
            editor.pending_edits.clear();
            editor.queue_edit(
                if focused == Some(ControlKey(entity.to_bits())) && dimension.is_some() {
                    TextEdit::SelectAll
                } else if focused == Some(ControlKey(entity.to_bits())) {
                    TextEdit::TextEnd(false)
                } else {
                    // External updates to an unfocused number must reveal its
                    // sign and leading digits, rather than scrolling to its tail.
                    TextEdit::TextStart(false)
                },
            );
            field.baseline.clone_from(value);
            field.binding = control.binding;
            field.queued = None;
            field.undo.clear();
            field.redo.clear();
            field.composition = None;
            revision.0 = revision.0.wrapping_add(1);
        }
        let display = if control.visible {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let edge = BorderColor::all(if focused == Some(ControlKey(entity.to_bits())) {
            field.theme.accent
        } else {
            field.theme.edge
        });
        if *border != edge {
            *border = edge;
        }
        let focus_ring = if !control.disabled && focused == Some(ControlKey(entity.to_bits())) {
            field.theme.accent
        } else {
            Color::NONE
        };
        if let Some(mut outline) = outline {
            if outline.color != focus_ring {
                outline.color = focus_ring;
            }
        }
    }
}

fn update_ime(
    handle: Res<NativeInterfaceHandle>,
    focus: Option<Res<bevy::input_focus::InputFocus>>,
    standard_fields: Query<(), With<bevy::ui_widgets::TextInput>>,
    fields: ImeFieldQuery,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    scale: Res<UiScale>,
    mut candidate: Option<ResMut<ime_popup::ImeCandidateWindow>>,
) {
    if focus
        .as_deref()
        .and_then(bevy::input_focus::InputFocus::get)
        .is_some_and(|entity| standard_fields.get(entity).is_ok())
    {
        if let Some(candidate) = candidate.as_mut() {
            candidate.enabled = false;
            candidate.popup = None;
        }
        return;
    }
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    let focused = handle
        .focused_key()
        .and_then(|key| fields.get(Entity::from_bits(key.0)).ok());
    window.ime_enabled = focused.is_some();
    let Some((editor, node, transform)) = focused else {
        if let Some(candidate) = candidate.as_mut() {
            candidate.enabled = false;
            candidate.popup = None;
            candidate.field_pixels = None;
        }
        return;
    };
    let monitor_scale = window.scale_factor();
    let area = editor.editor.ime_cursor_area();
    let Some(placement) = ime_popup::place_ime_popup(ime_popup::ImePopupInput {
        caret: ime_popup::PixelRect {
            x: area.x0 as f32,
            y: area.y0 as f32,
            width: area.width() as f32,
            height: area.height() as f32,
        },
        content_min: node.content_box().min,
        scroll: editor.viewport.offset,
        transform: transform.affine(),
        field_local: node.border_box(),
        inverse_scale_factor: node.inverse_scale_factor(),
        ui_scale: scale.0,
        monitor_scale,
    }) else {
        return;
    };
    window.ime_position = Vec2::new(placement.popup.origin[0], placement.popup.origin[1]);
    if let Some(candidate) = candidate.as_mut() {
        candidate.enabled = true;
        candidate.popup = Some(placement.popup);
        candidate.field_pixels = Some(placement.field_pixels);
        candidate.scale_factor = monitor_scale;
    }
}

#[cfg(test)]
mod ime_diagnostic_tests {
    use super::super::ime_diagnostics::Trace;
    use super::*;
    use serde_json::json;

    #[test]
    fn opt_in_configuration_distinguishes_missing_layout_without_fabricating_ime() {
        let (mut app, handle, entity) = super::tests::editor_fixture();
        handle.shared.lock().unwrap().ime_diagnostics = Some(Trace::for_test());
        app.init_resource::<UiScale>();
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        app.world_mut().run_system_cached(update_ime).unwrap();
        super::super::ime_diagnostics::observe_configuration(app.world_mut());
        let snapshot = handle.inspect().unwrap();
        let configuration = &snapshot["ime_diagnostics"]["configuration"];
        assert_eq!(configuration["window"]["ime_enabled"], true);
        assert_eq!(
            configuration["native_field_components"]["render_target"],
            true
        );
        assert_eq!(configuration["control_key"], entity.to_bits());
        assert_eq!(snapshot["ime_diagnostics"]["events"], json!([]));
        app.world_mut()
            .entity_mut(entity)
            .remove::<ComputedUiRenderTargetInfo>();
        app.world_mut().run_system_cached(update_ime).unwrap();
        super::super::ime_diagnostics::observe_configuration(app.world_mut());
        let snapshot = handle.inspect().unwrap();
        assert_eq!(
            snapshot["ime_diagnostics"]["configuration"]["window"]["ime_enabled"],
            false
        );
        assert_eq!(
            snapshot["ime_diagnostics"]["configuration"]["native_field_components"]
                ["render_target"],
            false
        );
        let key = WindowEvent::KeyboardInput(KeyboardInput {
            key_code: KeyCode::ArrowRight,
            logical_key: Key::ArrowRight,
            text: None,
            state: ButtonState::Pressed,
            repeat: false,
            window,
        });
        before_window_input(app.world_mut(), &handle, &key, None, default()).unwrap();
        let snapshot = handle.inspect().unwrap();
        assert_eq!(
            snapshot["ime_diagnostics"]["keyboard"][0]["key_code"],
            "ArrowRight"
        );
        assert_eq!(
            snapshot["ime_diagnostics"]["keyboard"][0]["window"]["ime_enabled"],
            false
        );
        assert_eq!(snapshot["ime_diagnostics"]["events"], json!([]));
        assert!(snapshot["ime_diagnostics"]["current"].is_null());
    }

    #[test]
    fn opt_in_ime_observer_follows_accepted_editor_events_and_rejects_retired_owner() {
        let (mut app, handle, entity) = super::tests::editor_fixture();
        assert!(handle.inspect().unwrap().get("ime_diagnostics").is_none());
        handle.shared.lock().unwrap().ime_diagnostics = Some(Trace::for_test());
        let owner = handle.frame().unwrap().context;
        let key = WindowEvent::KeyboardInput(KeyboardInput {
            key_code: KeyCode::ArrowRight,
            logical_key: Key::ArrowRight,
            text: None,
            state: ButtonState::Pressed,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        before_window_input(app.world_mut(), &handle, &key, None, default()).unwrap();
        assert_eq!(
            handle.inspect().unwrap()["ime_diagnostics"]["events"],
            json!([])
        );
        for event in [
            WindowEvent::Ime(Ime::Preedit {
                window: Entity::PLACEHOLDER,
                value: "はる".into(),
                cursor: Some((6, 6)),
            }),
            WindowEvent::Ime(Ime::Commit {
                window: Entity::PLACEHOLDER,
                value: "はる".into(),
            }),
        ] {
            before_window_input(app.world_mut(), &handle, &event, None, default()).unwrap();
        }
        let snapshot = handle.inspect().unwrap();
        let events = snapshot["ime_diagnostics"]["events"].as_array().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["kind"], "preedit");
        assert_eq!(events[0]["cursor"], json!([6, 6]));
        assert_eq!(events[0]["composing"], true);
        assert_eq!(events[1]["kind"], "commit");
        assert_eq!(events[1]["composing"], false);
        assert_eq!(
            events[1]["context"],
            json!({
                "window_id":owner.window_id, "document_id":owner.document_id, "epoch":owner.epoch,
            })
        );
        assert_eq!(events[1]["control_key"], entity.to_bits());
        let mut replacement = handle.frame().unwrap();
        replacement.context.epoch += 1;
        handle.present(replacement).unwrap();
        let late = WindowEvent::Ime(Ime::Commit {
            window: Entity::PLACEHOLDER,
            value: "late".into(),
        });
        assert!(before_window_input(app.world_mut(), &handle, &late, None, default()).unwrap());
        assert_eq!(
            handle
                .shared
                .lock()
                .unwrap()
                .ime_diagnostics
                .as_ref()
                .unwrap()
                .snapshot()["events"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            app.world()
                .get::<EditableText>(entity)
                .unwrap()
                .value()
                .to_string(),
            "12はる"
        );
    }
}
