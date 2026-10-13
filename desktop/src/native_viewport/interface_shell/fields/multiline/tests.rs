use super::super::tests::editor_fixture;
use super::*;

fn enter_event() -> WindowEvent {
    WindowEvent::KeyboardInput(KeyboardInput {
        key_code: KeyCode::Enter,
        logical_key: Key::Enter,
        text: Some("\r".into()),
        state: ButtonState::Pressed,
        repeat: false,
        window: Entity::PLACEHOLDER,
    })
}
fn value(world: &World, entity: Entity) -> String {
    world
        .get::<EditableText>(entity)
        .unwrap()
        .value()
        .to_string()
}

#[test]
fn multiline_is_opt_in_and_enter_and_shift_enter_keep_literal_source_in_the_existing_editor() {
    let (mut app, handle, entity) = editor_fixture();
    assert!(!enter(app.world_mut(), entity, Modifiers::default()).unwrap());
    assert_eq!(value(app.world(), entity), "12");
    enable(app.world_mut(), entity).unwrap();
    assert_eq!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .visible_lines,
        None
    );
    assert!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .allow_newlines
    );
    for shift in [false, true] {
        assert!(before_window_input(
            app.world_mut(),
            &handle,
            &enter_event(),
            None,
            Modifiers { shift, ..default() }
        )
        .unwrap());
    }
    assert_eq!(value(app.world(), entity), "12\n\n");
    assert!(
        handle.take_actions().unwrap().is_empty(),
        "Newlines must not commit or submit NC source"
    );
    assert_eq!(
        app.world().get::<NativeTextField>(entity).unwrap().baseline,
        "12"
    );
    apply_edit(
        app.world_mut(),
        entity,
        TextEdit::Insert("G1 X20 ; Café 零件\r\nM30\n".into()),
    )
    .unwrap();
    assert_eq!(
        value(app.world(), entity),
        "12\n\nG1 X20 ; Café 零件\r\nM30\n"
    );
    history_edit(app.world_mut(), entity, false).unwrap();
    assert_eq!(value(app.world(), entity), "12\n\n");
    history_edit(app.world_mut(), entity, true).unwrap();
    assert_eq!(
        value(app.world(), entity),
        "12\n\nG1 X20 ; Café 零件\r\nM30\n"
    );
}

#[test]
fn multiline_control_enter_does_not_submit_and_modified_enter_keeps_existing_behavior() {
    let (mut app, handle, entity) = editor_fixture();
    enable(app.world_mut(), entity).unwrap();
    let owner = handle.frame().unwrap().context;
    let action = handle
        .resolve_input(
            ControlKey(entity.to_bits()),
            ControlInput::Key(limo_cad_interface::KeyChord::plain("Enter")),
            &owner,
        )
        .unwrap();
    assert!(adapt_control_input(app.world_mut(), &handle, &action)
        .unwrap()
        .is_none());
    assert_eq!(value(app.world(), entity), "12\n");
    for modifiers in [
        Modifiers {
            ctrl: true,
            ..default()
        },
        Modifiers {
            meta: true,
            ..default()
        },
        Modifiers {
            alt: true,
            ..default()
        },
    ] {
        assert!(!enter(app.world_mut(), entity, modifiers).unwrap());
    }
    assert_eq!(value(app.world(), entity), "12\n");
}

#[test]
fn multiline_ime_candidate_confirmation_does_not_insert_a_newline_or_commit_a_draft() {
    let (mut app, handle, entity) = editor_fixture();
    enable(app.world_mut(), entity).unwrap();
    let window = Entity::PLACEHOLDER;
    let preedit = WindowEvent::Ime(Ime::Preedit {
        window,
        value: "你好".into(),
        cursor: Some((0, 6)),
    });
    assert!(before_window_input(
        app.world_mut(),
        &handle,
        &preedit,
        None,
        Modifiers::default()
    )
    .unwrap());
    assert!(before_window_input(
        app.world_mut(),
        &handle,
        &enter_event(),
        None,
        Modifiers::default()
    )
    .unwrap());
    assert!(enter(app.world_mut(), entity, Modifiers::default()).unwrap());
    assert!(app
        .world()
        .get::<EditableText>(entity)
        .unwrap()
        .is_composing());
    assert!(handle.take_actions().unwrap().is_empty());
    let commit = WindowEvent::Ime(Ime::Commit {
        window,
        value: "你好".into(),
    });
    assert!(before_window_input(
        app.world_mut(),
        &handle,
        &commit,
        None,
        Modifiers::default()
    )
    .unwrap());
    assert_eq!(value(app.world(), entity), "12你好");
    assert!(before_window_input(
        app.world_mut(),
        &handle,
        &enter_event(),
        None,
        Modifiers::default()
    )
    .unwrap());
    assert_eq!(value(app.world(), entity), "12你好\n");
}

#[test]
fn multiline_read_only_source_can_navigate_but_cannot_insert_newlines() {
    let (mut app, _, entity) = editor_fixture();
    enable(app.world_mut(), entity).unwrap();
    if let Field::Text { read_only, .. } = &mut app
        .world_mut()
        .get_mut::<InterfaceControl>(entity)
        .unwrap()
        .field
    {
        *read_only = true;
    }
    assert!(enter(app.world_mut(), entity, Modifiers::default()).unwrap());
    assert_eq!(value(app.world(), entity), "12");
    apply_edit(app.world_mut(), entity, TextEdit::TextStart(false)).unwrap();
    assert_eq!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .editor
            .raw_selection()
            .text_range(),
        0..0
    );
}

#[test]
fn multiline_caret_and_wheel_scroll_in_both_axes_without_modifying_source() {
    let (mut app, handle, entity) = editor_fixture();
    enable(app.world_mut(), entity).unwrap();
    let bounds = handle
        .read_surface(|_, frame| {
            frame
                .controls
                .iter()
                .find(|control| control.key == ControlKey(entity.to_bits()))
                .unwrap()
                .bounds
        })
        .unwrap();
    let inside = Vec2::new(
        (bounds.x + bounds.width * 0.5) as f32,
        (bounds.y + bounds.height * 0.5) as f32,
    );
    assert_eq!(
        handle.hit_key(inside.as_dvec2().to_array()),
        Some(ControlKey(entity.to_bits()))
    );
    app.world_mut()
        .run_system_cached(bevy::ui::widget::update_editable_text_styles)
        .unwrap();
    app.world_mut()
        .run_system_cached(bevy::ui::widget::sync_editable_text_viewports)
        .unwrap();
    let source = (0..40)
        .map(|i| format!("N{i} G1 X123.456 Y234.567 Z345.678 ; long source line\n"))
        .collect::<String>();
    apply_edit(app.world_mut(), entity, TextEdit::SelectAll).unwrap();
    apply_edit(
        app.world_mut(),
        entity,
        TextEdit::Insert(source.clone().into()),
    )
    .unwrap();
    apply_edit(app.world_mut(), entity, TextEdit::Left(false)).unwrap();
    let offset = app
        .world()
        .get::<EditableText>(entity)
        .unwrap()
        .viewport
        .offset;
    assert!(
        offset.x > 0. && offset.y > 0.,
        "Caret reveal must scroll both axes: {offset:?}"
    );
    let wheel_event = MouseWheel {
        unit: MouseScrollUnit::Pixel,
        x: 100000.,
        y: 100000.,
        window: Entity::PLACEHOLDER,
        phase: bevy::input::touch::TouchPhase::Moved,
    };
    assert!(!wheel(
        app.world_mut(),
        &handle,
        entity,
        &wheel_event,
        Some(Vec2::new(900., 700.)),
        Modifiers::default()
    )
    .unwrap());
    assert_eq!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .viewport
            .offset,
        offset
    );
    assert!(wheel(
        app.world_mut(),
        &handle,
        entity,
        &wheel_event,
        Some(inside),
        Modifiers::default()
    )
    .unwrap());
    assert_eq!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .viewport
            .offset,
        Vec2::ZERO
    );
    let line = MouseWheel {
        unit: MouseScrollUnit::Line,
        x: 0.,
        y: -2.,
        window: Entity::PLACEHOLDER,
        phase: bevy::input::touch::TouchPhase::Moved,
    };
    assert!(wheel(
        app.world_mut(),
        &handle,
        entity,
        &line,
        Some(inside),
        Modifiers {
            shift: true,
            ..default()
        }
    )
    .unwrap());
    let shifted = app
        .world()
        .get::<EditableText>(entity)
        .unwrap()
        .viewport
        .offset;
    assert!(
        shifted.x > 0. && shifted.y == 0.,
        "Shift+wheel scrolls source horizontally: {shifted:?}"
    );
    assert!(wheel(
        app.world_mut(),
        &handle,
        entity,
        &line,
        Some(inside),
        Modifiers::default()
    )
    .unwrap());
    assert!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .viewport
            .offset
            .y
            > 0.
    );
    assert_eq!(value(app.world(), entity), source);
    assert!(handle.take_actions().unwrap().is_empty());
}
