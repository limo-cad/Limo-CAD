use super::*;
use crate::native_viewport::winit_host::{prepare_native_input, HostInputState, NativeHostInput};

fn dispatch(app: &mut App, handle: &NativeInterfaceHandle, event: WindowEvent) -> NativeHostInput {
    app.init_resource::<HostInputState>();
    let mut input = NativeHostInput {
        ui_scale: 1.,
        context: handle.presented_context(),
        cursor: None,
        modifiers: Modifiers::default(),
        event,
        consumed: false,
        actions: Vec::new(),
    };
    prepare_native_input(app.world_mut(), handle, &mut input).unwrap();
    input
}

fn focused(focused: bool) -> WindowEvent {
    WindowEvent::WindowFocused(bevy::window::WindowFocused {
        window: Entity::PLACEHOLDER,
        focused,
    })
}

#[test]
fn window_reactivation_resumes_unicode_and_numeric_editing_at_the_same_caret() {
    for drawing in [false, true] {
        let (mut app, handle, entity) = editor_fixture();
        if drawing {
            drawing_field(&mut app, entity, 0);
        }
        let value = if drawing {
            "10.5"
        } else {
            "Café 零件 Ω 🦀"
        };
        apply_edit(app.world_mut(), entity, TextEdit::SelectAll).unwrap();
        apply_edit(app.world_mut(), entity, TextEdit::Insert(value.into())).unwrap();
        apply_edit(app.world_mut(), entity, TextEdit::TextEnd(false)).unwrap();
        let caret = *app
            .world()
            .get::<EditableText>(entity)
            .unwrap()
            .editor
            .raw_selection();
        let blur = dispatch(&mut app, &handle, focused(false));
        assert_eq!(handle.focused_key(), None);
        assert!(app.world().resource::<EditorSession>().active.is_none());
        assert_eq!(blur.actions.len(), 1);
        acknowledge_control_input(app.world_mut(), &blur.actions[0], true);
        // Bevy also reports keyboard focus loss; this must retain the original
        // return target rather than overwrite it with an empty focus.
        dispatch(
            &mut app,
            &handle,
            WindowEvent::KeyboardFocusLost(bevy::input::keyboard::KeyboardFocusLost),
        );
        dispatch(&mut app, &handle, focused(true));
        assert_eq!(handle.focused_key(), Some(ControlKey(entity.to_bits())));
        let editor = app.world().get::<EditableText>(entity).unwrap();
        assert_eq!(editor.value(), value);
        assert_eq!(editor.editor.raw_selection(), &caret);
        assert!(app.world().resource::<EditorSession>().active.is_some());
        assert!(handle.take_actions().unwrap().is_empty());
        assert!(before_window_input(
            app.world_mut(),
            &handle,
            &key(Key::Character("a".into()), Some("a")),
            None,
            Modifiers {
                ctrl: !cfg!(target_os = "macos"),
                meta: cfg!(target_os = "macos"),
                ..default()
            }
        )
        .unwrap());
        assert_eq!(
            app.world()
                .get::<EditableText>(entity)
                .unwrap()
                .editor
                .raw_selection()
                .text_range(),
            0..value.len()
        );
    }
}

#[test]
fn window_reactivation_discards_retired_disabled_and_explicitly_blurred_targets() {
    for change in [
        "binding", "hidden", "disabled", "removed", "document", "modal", "blur",
    ] {
        let (mut app, handle, entity) = editor_fixture();
        dispatch(&mut app, &handle, focused(false));
        match change {
            "binding" => {
                app.world_mut()
                    .get_mut::<InterfaceControl>(entity)
                    .unwrap()
                    .binding += 1
            }
            "hidden" => {
                app.world_mut()
                    .get_mut::<InterfaceControl>(entity)
                    .unwrap()
                    .visible = false
            }
            "disabled" => {
                app.world_mut()
                    .get_mut::<InterfaceControl>(entity)
                    .unwrap()
                    .disabled = true
            }
            "removed" => {
                app.world_mut().despawn(entity);
            }
            "document" => {
                let mut frame = handle.frame().unwrap();
                frame.context.document_id = "replacement".into();
                handle.present(frame).unwrap();
            }
            "modal" => {
                let mut frame = handle.frame().unwrap();
                frame.modal_stack.push("Viewport".into());
                handle.present(frame.clone()).unwrap();
                app.update();
                frame.modal_stack.clear();
                handle.present(frame).unwrap();
            }
            "blur" => handle.blur(),
            _ => unreachable!(),
        }
        app.update();
        dispatch(&mut app, &handle, focused(true));
        assert_eq!(handle.focused_key(), None, "{change}");
        assert!(
            app.world().resource::<EditorSession>().active.is_none(),
            "{change}"
        );
    }
}

#[test]
fn window_reactivation_cancels_ime_preedit_and_keeps_the_committed_selection() {
    let (mut app, handle, entity) = editor_fixture();
    apply_edit(app.world_mut(), entity, TextEdit::SelectAll).unwrap();
    before_window_input(
        app.world_mut(),
        &handle,
        &WindowEvent::Ime(Ime::Preedit {
            window: Entity::PLACEHOLDER,
            value: "はる".into(),
            cursor: Some((6, 6)),
        }),
        None,
        Modifiers::default(),
    )
    .unwrap();
    dispatch(&mut app, &handle, focused(false));
    dispatch(&mut app, &handle, focused(true));
    assert_eq!(handle.focused_key(), Some(ControlKey(entity.to_bits())));
    let editor = app.world().get::<EditableText>(entity).unwrap();
    assert_eq!(editor.value(), "12");
    assert!(!editor.is_composing());
    assert_eq!(editor.editor.raw_selection().text_range(), 0..2);
    assert!(handle.take_actions().unwrap().is_empty());
}

#[test]
fn window_reactivation_keeps_new_focus_and_never_resumes_pointer_capture() {
    let (mut app, handle, entity) = editor_fixture();
    let button = app
        .world_mut()
        .spawn((
            InterfaceControl::button("Viewport", "Apply"),
            ComputedNode {
                size: Vec2::new(80., 24.),
                ..default()
            },
            UiGlobalTransform::from_translation(Vec2::new(160., 42.)),
            bevy::ui::ComputedStackIndex(2),
            InheritedVisibility::VISIBLE,
        ))
        .id();
    app.update();
    handle
        .pointer(
            super::super::super::PointerPhase::Down,
            [140., 140.],
            super::super::super::PointerButton::Primary,
        )
        .unwrap();
    assert_eq!(handle.focused_key(), Some(ControlKey(entity.to_bits())));
    assert!(handle.has_capture());
    dispatch(&mut app, &handle, focused(false));
    assert!(!handle.has_capture());
    let action = handle
        .resolve_retained(ControlKey(button.to_bits()))
        .unwrap();
    handle.prepare_activation(&action).unwrap();
    dispatch(&mut app, &handle, focused(true));
    assert_eq!(handle.focused_key(), Some(ControlKey(button.to_bits())));
    assert!(!handle.has_capture());
    assert!(app.world().resource::<EditorSession>().active.is_none());
    assert!(handle.take_actions().unwrap().is_empty());
}
