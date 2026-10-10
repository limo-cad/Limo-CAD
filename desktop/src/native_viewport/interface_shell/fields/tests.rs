use super::*;
use crate::native_viewport::interface_shell::tests::fixture;

mod window_focus;

fn drawing_field(app: &mut App, entity: Entity, index: usize) {
    app.world_mut().entity_mut(entity).insert(DrawingDimension {
        generation: 7,
        index,
    });
    app.init_resource::<bevy::input_focus::InputFocus>();
    app.world_mut()
        .entity_mut(entity)
        .insert((Node::default(), BorderColor::default()));
}

#[test]
fn drawing_enter_queues_the_visible_value_before_confirming_the_shape() {
    let (mut app, handle, entity) = editor_fixture_with_submit(true);
    drawing_field(&mut app, entity, 0);
    apply_edit(app.world_mut(), entity, TextEdit::SelectAll).unwrap();
    apply_edit(app.world_mut(), entity, TextEdit::Insert("10".into())).unwrap();
    assert!(before_window_input(
        app.world_mut(),
        &handle,
        &key(Key::Enter, None),
        None,
        default()
    )
    .unwrap());
    let actions = handle.take_actions().unwrap();
    assert_eq!(actions.len(), 2);
    assert_eq!(
        actions[0].control.input,
        ControlInput::SetValue("10".into())
    );
    assert_eq!(
        actions[1].control.input,
        ControlInput::Key(limo_cad_interface::KeyChord::plain("Enter"))
    );
    acknowledge_control_input(app.world_mut(), &actions[0], true);
    assert!(before_window_input(
        app.world_mut(),
        &handle,
        &key(Key::Enter, None),
        None,
        default()
    )
    .unwrap());
    let actions = handle.take_actions().unwrap();
    assert_eq!(
        actions.len(),
        1,
        "An already committed number must still confirm"
    );
    assert_eq!(
        actions[0].control.input,
        ControlInput::Key(limo_cad_interface::KeyChord::plain("Enter"))
    );
}

#[test]
fn drawing_preview_releases_undo_and_redo_to_document_history() {
    for redo in [false, true] {
        let (mut app, handle, entity) = editor_fixture();
        drawing_field(&mut app, entity, 0);
        let modifiers = Modifiers {
            meta: cfg!(target_os = "macos"),
            ctrl: !cfg!(target_os = "macos"),
            shift: redo,
            ..default()
        };
        assert!(!before_window_input(
            app.world_mut(),
            &handle,
            &key(Key::Character("z".into()), None),
            None,
            modifiers
        )
        .unwrap());
        assert_eq!(handle.focused_key(), None);
        assert!(app.world().resource::<EditorSession>().active.is_none());
        assert!(handle.take_actions().unwrap().is_empty());
    }
}

#[test]
fn drawing_number_keeps_local_undo_and_redo_until_the_edit_is_committed() {
    let (mut app, handle, entity) = editor_fixture();
    drawing_field(&mut app, entity, 0);
    apply_edit(app.world_mut(), entity, TextEdit::SelectAll).unwrap();
    apply_edit(app.world_mut(), entity, TextEdit::Insert("3".into())).unwrap();
    for (redo, expected) in [(false, "12"), (true, "3")] {
        assert!(before_window_input(
            app.world_mut(),
            &handle,
            &key(Key::Character("z".into()), None),
            None,
            Modifiers {
                meta: cfg!(target_os = "macos"),
                ctrl: !cfg!(target_os = "macos"),
                shift: redo,
                ..default()
            }
        )
        .unwrap());
        assert_eq!(
            app.world().get::<EditableText>(entity).unwrap().value(),
            expected
        );
        assert_eq!(handle.focused_key(), Some(ControlKey(entity.to_bits())));
        assert!(handle.take_actions().unwrap().is_empty());
    }
}

fn key(key: Key, text: Option<&str>) -> WindowEvent {
    WindowEvent::KeyboardInput(KeyboardInput {
        key_code: KeyCode::Unidentified(bevy::input::keyboard::NativeKeyCode::Unidentified),
        logical_key: key,
        state: ButtonState::Pressed,
        text: text.map(Into::into),
        repeat: false,
        window: Entity::PLACEHOLDER,
    })
}

#[test]
fn external_field_updates_reveal_leading_digits_without_changing_precision() {
    for focused in [false, true] {
        let (mut app, handle, entity) = editor_fixture();
        app.init_resource::<bevy::input_focus::InputFocus>();
        if !focused {
            handle.blur();
            after_window_input(app.world_mut(), &handle).unwrap();
        }
        let value = "-5.00001335144043";
        app.world_mut()
            .get_mut::<InterfaceControl>(entity)
            .unwrap()
            .field = Field::Text {
            value: value.into(),
            read_only: false,
            selection: None,
        };
        app.world_mut()
            .run_system_cached(synchronize_fields)
            .unwrap();
        flush_edits(app.world_mut()).unwrap();
        let editor = app.world().get::<EditableText>(entity).unwrap();
        assert_eq!(editor.value(), value);
        let cursor = if focused { value.len() } else { 0 };
        assert_eq!(editor.editor.raw_selection().text_range(), cursor..cursor);
        assert!(!has_uncommitted_edit(app.world(), entity));
        assert!(handle.take_actions().unwrap().is_empty());
    }
}

#[test]
fn requested_measurement_focus_selects_the_value_once_without_replacing_typed_edits() {
    let (mut app, handle, entity) = editor_fixture();
    handle.blur();
    after_window_input(app.world_mut(), &handle).unwrap();
    let owner = handle.frame().unwrap().context;
    request_focus(app.world_mut(), entity, &owner);
    apply_requested_focus(app.world_mut());
    assert_eq!(handle.focused_key(), Some(ControlKey(entity.to_bits())));
    assert_eq!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .editor
            .raw_selection()
            .text_range(),
        0..2
    );
    for digit in ["3", "5"] {
        assert!(before_window_input(
            app.world_mut(),
            &handle,
            &key(Key::Character(digit.into()), Some(digit)),
            None,
            default()
        )
        .unwrap());
        apply_requested_focus(app.world_mut());
    }
    assert_eq!(
        app.world().get::<EditableText>(entity).unwrap().value(),
        "35"
    );
    assert!(has_uncommitted_edit(app.world(), entity));
    assert!(handle.take_actions().unwrap().is_empty());
}

#[test]
fn drawing_autofocus_replaces_the_latest_preview_with_the_first_typed_number() {
    let (mut app, handle, entity) = editor_fixture();
    drawing_field(&mut app, entity, 0);
    handle.blur();
    after_window_input(app.world_mut(), &handle).unwrap();
    let owner = handle.frame().unwrap().context;
    request_focus(app.world_mut(), entity, &owner);
    apply_requested_focus(app.world_mut());
    assert_eq!(handle.focused_key(), Some(ControlKey(entity.to_bits())));
    app.world_mut()
        .get_mut::<InterfaceControl>(entity)
        .unwrap()
        .field = Field::Text {
        value: "18.500".into(),
        read_only: false,
        selection: None,
    };
    app.world_mut()
        .run_system_cached(synchronize_fields)
        .unwrap();
    flush_edits(app.world_mut()).unwrap();
    assert_eq!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .editor
            .raw_selection()
            .text_range(),
        0..6
    );
    assert!(!has_uncommitted_edit(app.world(), entity));
    assert!(before_window_input(
        app.world_mut(),
        &handle,
        &key(Key::Character("3".into()), Some("3")),
        None,
        default()
    )
    .unwrap());
    assert_eq!(
        app.world().get::<EditableText>(entity).unwrap().value(),
        "3"
    );
    assert!(has_uncommitted_edit(app.world(), entity));
    assert!(handle.take_actions().unwrap().is_empty());
}

#[test]
fn drawing_autofocus_waits_for_the_original_binding_to_be_published() {
    let (mut app, handle, entity) = editor_fixture();
    drawing_field(&mut app, entity, 0);
    handle.blur();
    after_window_input(app.world_mut(), &handle).unwrap();
    app.world_mut()
        .get_mut::<InterfaceControl>(entity)
        .unwrap()
        .binding += 1;
    let owner = handle.frame().unwrap().context;
    request_focus(app.world_mut(), entity, &owner);
    apply_requested_focus(app.world_mut());
    assert_eq!(handle.focused_key(), None);
    assert!(app.world().resource::<RequestedFocus>().0.is_some());
    app.update();
    app.world_mut()
        .run_system_cached(synchronize_fields)
        .unwrap();
    apply_requested_focus(app.world_mut());
    assert_eq!(handle.focused_key(), Some(ControlKey(entity.to_bits())));
    assert!(app.world().resource::<RequestedFocus>().0.is_none());
    assert_eq!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .editor
            .raw_selection()
            .text_range(),
        0..2
    );
    assert!(handle.take_actions().unwrap().is_empty());
}

#[test]
fn drawing_autofocus_rejects_rebound_fields_and_replaced_documents() {
    for replace_document in [false, true] {
        let (mut app, handle, entity) = editor_fixture();
        drawing_field(&mut app, entity, 0);
        handle.blur();
        after_window_input(app.world_mut(), &handle).unwrap();
        let owner = handle.frame().unwrap().context;
        request_focus(app.world_mut(), entity, &owner);
        if replace_document {
            let mut frame = handle.frame().unwrap();
            frame.context.epoch += 1;
            handle.present(frame).unwrap();
        } else {
            app.world_mut()
                .get_mut::<InterfaceControl>(entity)
                .unwrap()
                .binding += 1;
        }
        app.update();
        apply_requested_focus(app.world_mut());
        assert_eq!(handle.focused_key(), None);
        assert!(app.world().resource::<EditorSession>().active.is_none());
        assert!(handle.take_actions().unwrap().is_empty());
    }
}

#[test]
fn drawing_tab_commits_then_cycles_dimensions_without_visiting_toolbar_controls() {
    let (mut app, handle, first) = editor_fixture();
    drawing_field(&mut app, first, 0);
    let theme = ViewportUiTheme::from_palette(&default());
    let mut control = InterfaceControl::button("Viewport", "Angle");
    control.field = Field::Text {
        value: "45.000".into(),
        read_only: false,
        selection: None,
    };
    let second = spawn_text_field(
        &mut app.world_mut().commands(),
        Entity::PLACEHOLDER,
        Node::default(),
        control,
        theme,
        &ViewportUiAssets::default(),
    )
    .unwrap();
    app.world_mut().flush();
    app.world_mut().entity_mut(second).insert((
        ComputedNode {
            size: Vec2::new(80., 24.),
            inverse_scale_factor: 1.,
            ..default()
        },
        UiGlobalTransform::from_translation(Vec2::new(160., 42.)),
        bevy::ui::ComputedStackIndex(2),
        InheritedVisibility::VISIBLE,
    ));
    drawing_field(&mut app, second, 1);
    app.update();
    app.world_mut()
        .run_system_cached(bevy::ui::widget::update_editable_text_styles)
        .unwrap();
    app.world_mut()
        .run_system_cached(bevy::ui::widget::update_editable_text_layout)
        .unwrap();
    apply_edit(app.world_mut(), first, TextEdit::SelectAll).unwrap();
    for digit in ["3", "0"] {
        assert!(before_window_input(
            app.world_mut(),
            &handle,
            &key(Key::Character(digit.into()), Some(digit)),
            None,
            default()
        )
        .unwrap());
    }
    assert!(before_window_input(
        app.world_mut(),
        &handle,
        &key(Key::Tab, None),
        None,
        default()
    )
    .unwrap());
    assert_eq!(handle.focused_key(), Some(ControlKey(second.to_bits())));
    let commits = handle.take_actions().unwrap();
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].control.key, ControlKey(first.to_bits()));
    assert_eq!(
        commits[0].control.input,
        ControlInput::SetValue("30".into())
    );
    prepare_activation(app.world(), &handle, &commits[0]).unwrap();
    acknowledge_control_input(app.world_mut(), &commits[0], true);
    after_window_input(app.world_mut(), &handle).unwrap();
    assert_eq!(handle.focused_key(), Some(ControlKey(second.to_bits())));
    assert_eq!(
        app.world()
            .get::<EditableText>(second)
            .unwrap()
            .editor
            .raw_selection()
            .text_range(),
        0..6
    );
    for digit in ["6", "0"] {
        assert!(before_window_input(
            app.world_mut(),
            &handle,
            &key(Key::Character(digit.into()), Some(digit)),
            None,
            default()
        )
        .unwrap());
    }
    assert_eq!(
        app.world().get::<EditableText>(second).unwrap().value(),
        "60"
    );
    assert_eq!(
        app.world().get::<EditableText>(first).unwrap().value(),
        "30"
    );
    assert!(before_window_input(
        app.world_mut(),
        &handle,
        &key(Key::Tab, None),
        None,
        Modifiers {
            shift: true,
            ..default()
        }
    )
    .unwrap());
    assert_eq!(handle.focused_key(), Some(ControlKey(first.to_bits())));
    let commits = handle.take_actions().unwrap();
    assert_eq!(commits.len(), 1);
    assert_eq!(
        commits[0].control.input,
        ControlInput::SetValue("60".into())
    );
    prepare_activation(app.world(), &handle, &commits[0]).unwrap();
    acknowledge_control_input(app.world_mut(), &commits[0], true);
    after_window_input(app.world_mut(), &handle).unwrap();
    assert_eq!(handle.focused_key(), Some(ControlKey(first.to_bits())));
}

#[test]
fn focusing_a_text_field_does_not_submit_a_missing_value_to_its_form() {
    let (mut app, handle, entity) = editor_fixture();
    let owner = handle.frame().unwrap().context;
    for input in [ControlInput::Click, ControlInput::DoubleClick] {
        let action = handle
            .resolve_input(ControlKey(entity.to_bits()), input, &owner)
            .unwrap();
        assert!(adapt_control_input(app.world_mut(), &handle, &action)
            .unwrap()
            .is_none());
    }
    assert_eq!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .value()
            .to_string(),
        "12"
    );
    assert!(handle.take_actions().unwrap().is_empty());
}

#[test]
fn modifier_and_shortcut_keys_do_not_submit_missing_values_to_text_forms() {
    let (mut app, handle, entity) = editor_fixture();
    let owner = handle.frame().unwrap().context;
    let modifiers = Modifiers {
        ctrl: !cfg!(target_os = "macos"),
        meta: cfg!(target_os = "macos"),
        ..default()
    };
    for key in ["Control", "Meta", "Alt", "Shift", "a", "c", "v"] {
        let action = handle
            .resolve_input(
                ControlKey(entity.to_bits()),
                ControlInput::Key(limo_cad_interface::KeyChord {
                    key: key.into(),
                    ctrl: modifiers.ctrl,
                    meta: modifiers.meta,
                    ..default()
                }),
                &owner,
            )
            .unwrap();
        assert!(adapt_control_input(app.world_mut(), &handle, &action)
            .unwrap()
            .is_none());
    }
    let select_all = WindowEvent::KeyboardInput(KeyboardInput {
        key_code: KeyCode::KeyA,
        logical_key: Key::Character("a".into()),
        text: Some("a".into()),
        state: ButtonState::Pressed,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
    assert!(before_window_input(app.world_mut(), &handle, &select_all, None, modifiers).unwrap());
    let editor = app.world().get::<EditableText>(entity).unwrap();
    assert_eq!(editor.value().to_string(), "12");
    assert_eq!(editor.editor.raw_selection().text_range(), 0..2);
    assert!(handle.take_actions().unwrap().is_empty());
    let escape = handle
        .resolve_input(
            ControlKey(entity.to_bits()),
            ControlInput::Key(limo_cad_interface::KeyChord::plain("Escape")),
            &owner,
        )
        .unwrap();
    assert_eq!(
        adapt_control_input(app.world_mut(), &handle, &escape).unwrap(),
        Some(escape)
    );
}

#[test]
fn command_select_all_never_inserts_its_letter_on_either_platform() {
    let (mut app, _, entity) = editor_fixture();
    for mac in [false, true] {
        let modifiers = Modifiers {
            ctrl: !mac,
            meta: mac,
            ..default()
        };
        let edit =
            logical_edit_for_platform(&Key::Character("a".into()), Some("a"), modifiers, mac)
                .unwrap();
        assert!(matches!(edit, TextEdit::SelectAll));
        apply_edit(app.world_mut(), entity, edit).unwrap();
        let editor = app.world().get::<EditableText>(entity).unwrap();
        assert_eq!(editor.value().to_string(), "12");
        assert_eq!(editor.editor.raw_selection().text_range(), 0..2);
        for (letter, expected) in [("c", 0), ("x", 1), ("v", 2)] {
            let edit = logical_edit_for_platform(
                &Key::Character(letter.into()),
                Some(letter),
                modifiers,
                mac,
            )
            .unwrap();
            assert!(matches!(
                (expected, edit),
                (0, TextEdit::Copy) | (1, TextEdit::Cut) | (2, TextEdit::Paste)
            ));
        }
    }
}

#[test]
fn edit_shortcuts_follow_latin_layouts_and_fall_back_for_non_latin_layouts() {
    let mut input = KeyboardInput {
        key_code: KeyCode::KeyA,
        logical_key: Key::Character("ф".into()),
        state: ButtonState::Pressed,
        text: Some("ф".into()),
        repeat: false,
        window: Entity::PLACEHOLDER,
    };
    let command = Modifiers {
        ctrl: !cfg!(target_os = "macos"),
        meta: cfg!(target_os = "macos"),
        ..default()
    };
    assert!(matches!(
        keyboard_edit(&input, command),
        Some(TextEdit::SelectAll)
    ));
    assert!(
        matches!(keyboard_edit(&input, Modifiers::default()), Some(TextEdit::Insert(value)) if value.as_str() == "ф")
    );
    input.logical_key = Key::Character("q".into());
    assert!(
        keyboard_edit(&input, command).is_none(),
        "AZERTY Q must not become Select All"
    );
    input.key_code = KeyCode::KeyQ;
    input.logical_key = Key::Character("a".into());
    assert!(matches!(
        keyboard_edit(&input, command),
        Some(TextEdit::SelectAll)
    ));
    input.key_code = KeyCode::KeyZ;
    input.logical_key = Key::Character("я".into());
    assert_eq!(shortcut_key(&input), Key::Character("z".into()));
    input.logical_key = Key::Character("@".into());
    input.text = Some("@".into());
    assert!(
        matches!(keyboard_edit(&input, Modifiers { ctrl: true, alt: true, alt_graph: true, ..default() }), Some(TextEdit::Insert(value)) if value.as_str() == "@")
    );
}

#[test]
fn submit_fields_commit_the_visible_buffer_before_forwarding_enter() {
    let (mut app, handle, entity) = editor_fixture_with_submit(true);
    apply_edit(app.world_mut(), entity, TextEdit::Insert("3".into())).unwrap();
    let owner = handle.frame().unwrap().context;
    let enter = handle
        .resolve_input(
            ControlKey(entity.to_bits()),
            ControlInput::Key(limo_cad_interface::KeyChord::plain("Enter")),
            &owner,
        )
        .unwrap();
    let value = adapt_control_input(app.world_mut(), &handle, &enter)
        .unwrap()
        .unwrap();
    assert_eq!(value.control.input, ControlInput::SetValue("123".into()));
    acknowledge_control_input(app.world_mut(), &value, true);
    let queued = handle.take_actions().unwrap();
    assert_eq!(queued.len(), 1);
    let submit = adapt_control_input(app.world_mut(), &handle, &queued[0])
        .unwrap()
        .unwrap();
    assert_eq!(
        submit.control.input,
        ControlInput::Key(limo_cad_interface::KeyChord::plain("Enter"))
    );
    assert!(handle.take_actions().unwrap().is_empty());
}

#[test]
fn cancelling_a_form_discards_its_buffer_without_blocking_the_next_action() {
    let (mut app, handle, entity) = editor_fixture();
    apply_edit(app.world_mut(), entity, TextEdit::Insert("invalid".into())).unwrap();
    app.world_mut().despawn(entity);
    app.update();
    assert!(commit_active(app.world_mut(), &handle).unwrap().is_none());
    after_window_input(app.world_mut(), &handle).unwrap();
    assert!(app.world().resource::<EditorSession>().active.is_none());
    assert!(handle.take_actions().unwrap().is_empty());
}

#[test]
fn mcp_backspace_edits_the_same_visible_buffer_and_enter_does_not_revert_it() {
    let (mut app, handle, entity) = editor_fixture();
    let owner = handle.frame().unwrap().context;
    let action = handle
        .resolve_input(
            ControlKey(entity.to_bits()),
            ControlInput::Key(limo_cad_interface::KeyChord::plain("Backspace")),
            &owner,
        )
        .unwrap();
    let normalized = adapt_control_input(app.world_mut(), &handle, &action)
        .unwrap()
        .unwrap();
    assert_eq!(normalized.control.input, ControlInput::SetValue("1".into()));
    acknowledge_control_input(app.world_mut(), &normalized, true);
    let enter = handle
        .resolve_input(
            ControlKey(entity.to_bits()),
            ControlInput::Key(limo_cad_interface::KeyChord::plain("Enter")),
            &owner,
        )
        .unwrap();
    assert!(adapt_control_input(app.world_mut(), &handle, &enter)
        .unwrap()
        .is_none());
    assert_eq!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .value()
            .to_string(),
        "1"
    );
}

#[test]
fn field_undo_redo_changes_only_its_draft_and_new_input_discards_redo() {
    let (mut app, _handle, entity) = editor_fixture();
    apply_edit(app.world_mut(), entity, TextEdit::Insert("3".into())).unwrap();
    history_edit(app.world_mut(), entity, false).unwrap();
    assert_eq!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .value()
            .to_string(),
        "12"
    );
    history_edit(app.world_mut(), entity, true).unwrap();
    assert_eq!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .value()
            .to_string(),
        "123"
    );
    history_edit(app.world_mut(), entity, false).unwrap();
    apply_edit(app.world_mut(), entity, TextEdit::Insert("4".into())).unwrap();
    history_edit(app.world_mut(), entity, true).unwrap();
    assert_eq!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .value()
            .to_string(),
        "124"
    );
    assert_eq!(
        app.world().get::<NativeTextField>(entity).unwrap().baseline,
        "12"
    );
}

pub(super) fn editor_fixture() -> (App, NativeInterfaceHandle, Entity) {
    editor_fixture_with_submit(false)
}

fn editor_fixture_with_submit(submit: bool) -> (App, NativeInterfaceHandle, Entity) {
    let (mut app, handle, entity, _) = fixture();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::text::TextPlugin,
    ))
    .init_resource::<EditorSession>();
    let value = "12".to_owned();
    let mut control = app.world_mut().get_mut::<InterfaceControl>(entity).unwrap();
    control.field = Field::Text {
        value: value.clone(),
        read_only: false,
        selection: None,
    };
    control.text_editing = true;
    control.role = "textbox".into();
    if submit {
        control
            .owned_keys
            .push(limo_cad_interface::KeyChord::plain("Enter"));
    }

    app.world_mut().entity_mut(entity).insert((
        EditableText::new(&value),
        InterfaceTextRevision::default(),
        ComputedUiRenderTargetInfo::default(),
        NativeTextField {
            baseline: value,
            queued: None,
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            composition: None,
            binding: 1,
            theme: ViewportUiTheme::from_palette(&default()),
        },
    ));
    app.update();
    app.world_mut()
        .run_system_cached(bevy::ui::widget::update_editable_text_styles)
        .unwrap();
    app.init_resource::<Assets<Image>>();
    app.world_mut()
        .run_system_cached(bevy::ui::widget::update_editable_text_layout)
        .unwrap();
    let action = handle
        .resolve_retained(ControlKey(entity.to_bits()))
        .unwrap();
    handle.prepare_activation(&action).unwrap();
    after_window_input(app.world_mut(), &handle).unwrap();
    (app, handle, entity)
}

#[test]
fn a_direct_mcp_value_updates_the_visible_editor_and_is_not_reverted_on_blur() {
    for value in ["36 mm", ""] {
        let (mut app, handle, entity) = editor_fixture();
        apply_edit(app.world_mut(), entity, TextEdit::SelectAll).unwrap();
        apply_edit(
            app.world_mut(),
            entity,
            TextEdit::ImeSetCompose {
                value: "\u{306f}\u{308b}".into(),
                cursor: None,
            },
        )
        .unwrap();
        let owner = handle.frame().unwrap().context;
        let action = handle
            .resolve_input(
                ControlKey(entity.to_bits()),
                ControlInput::SetValue(value.into()),
                &owner,
            )
            .unwrap();
        assert!(prepare_control_input(app.world_mut(), &handle, &action)
            .unwrap()
            .is_empty());
        acknowledge_control_input(app.world_mut(), &action, true);
        assert!(!app
            .world()
            .get::<EditableText>(entity)
            .unwrap()
            .is_composing());
        apply_edit(app.world_mut(), entity, TextEdit::clear_ime_compose()).unwrap();
        assert!(app
            .world()
            .get::<NativeTextField>(entity)
            .unwrap()
            .composition
            .is_none());
        assert_eq!(
            app.world()
                .get::<EditableText>(entity)
                .unwrap()
                .value()
                .to_string(),
            value
        );
        assert!(commit_active(app.world_mut(), &handle).unwrap().is_none());
        handle.blur();
        after_window_input(app.world_mut(), &handle).unwrap();
        assert!(handle.take_actions().unwrap().is_empty());
    }
}

#[test]
fn rejected_commit_stays_dirty_and_duplicate_queued_blur_is_not_accepted_early() {
    let (mut app, handle, entity) = editor_fixture();
    app.world_mut()
        .get_mut::<EditableText>(entity)
        .unwrap()
        .queue_edit(TextEdit::Insert("bad".into()));
    let commit = commit_active(app.world_mut(), &handle).unwrap().unwrap();
    assert_eq!(
        app.world().get::<NativeTextField>(entity).unwrap().baseline,
        "12"
    );
    assert!(commit_active(app.world_mut(), &handle).unwrap().is_none());
    acknowledge_control_input(app.world_mut(), &commit, false);
    let retry = commit_active(app.world_mut(), &handle).unwrap().unwrap();
    assert_eq!(retry.control.input, commit.control.input);
    acknowledge_control_input(app.world_mut(), &retry, true);
    assert!(commit_active(app.world_mut(), &handle).unwrap().is_none());
}

#[test]
fn unicode_editing_and_ime_commit_precede_enter_without_synthetic_keys() {
    let (mut app, handle, entity) = editor_fixture();
    let window = Entity::from_bits(900);
    let event = WindowEvent::Ime(Ime::Commit {
        window,
        value: "日本".into(),
    });
    assert!(
        before_window_input(app.world_mut(), &handle, &event, None, Modifiers::default()).unwrap()
    );
    let value = app
        .world()
        .get::<EditableText>(entity)
        .unwrap()
        .value()
        .to_string();
    assert_eq!(value, "12日本");
    app.world_mut()
        .get_mut::<EditableText>(entity)
        .unwrap()
        .queue_edit(TextEdit::Backspace);
    let commit = commit_active(app.world_mut(), &handle).unwrap().unwrap();
    assert_eq!(commit.control.input, ControlInput::SetValue("12日".into()));
}

#[test]
fn read_only_fields_keep_selection_but_reject_typing_and_ime() {
    let (mut app, handle, entity) = editor_fixture();
    if let Field::Text { read_only, .. } = &mut app
        .world_mut()
        .get_mut::<InterfaceControl>(entity)
        .unwrap()
        .field
    {
        *read_only = true;
    }
    let key = WindowEvent::KeyboardInput(KeyboardInput {
        key_code: bevy::input::keyboard::KeyCode::ArrowLeft,
        logical_key: Key::ArrowLeft,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
    before_window_input(
        app.world_mut(),
        &handle,
        &key,
        None,
        Modifiers {
            meta: cfg!(target_os = "macos"),
            shift: true,
            ..default()
        },
    )
    .unwrap();
    assert!(!app
        .world()
        .get::<EditableText>(entity)
        .unwrap()
        .editor
        .raw_selection()
        .text_range()
        .is_empty());
    for edit in [
        TextEdit::Insert("changed".into()),
        TextEdit::Backspace,
        TextEdit::ImeSetCompose {
            value: "日本".into(),
            cursor: None,
        },
        TextEdit::ImeCommit {
            value: "日本".into(),
        },
    ] {
        apply_edit(app.world_mut(), entity, edit).unwrap();
    }
    let editor = app.world().get::<EditableText>(entity).unwrap();
    assert_eq!(editor.value().to_string(), "12");
    assert!(!editor.is_composing());
    assert!(app
        .world()
        .get::<NativeTextField>(entity)
        .unwrap()
        .undo
        .is_empty());
}

#[test]
fn ime_checkpoint_cannot_restore_text_across_a_rebound_field() {
    let (mut app, handle, entity) = editor_fixture();
    app.init_resource::<bevy::input_focus::InputFocus>();
    app.world_mut()
        .entity_mut(entity)
        .insert(BorderColor::default());
    apply_edit(app.world_mut(), entity, TextEdit::SelectAll).unwrap();
    apply_edit(
        app.world_mut(),
        entity,
        TextEdit::ImeSetCompose {
            value: "\u{306f}\u{308b}".into(),
            cursor: None,
        },
    )
    .unwrap();
    {
        let mut control = app.world_mut().get_mut::<InterfaceControl>(entity).unwrap();
        control.binding += 1;
        control.field = Field::Text {
            value: "rebound".into(),
            read_only: false,
            selection: None,
        };
    }
    app.world_mut()
        .run_system_cached(synchronize_fields)
        .unwrap();
    app.update();
    let field = app.world().get::<NativeTextField>(entity).unwrap();
    assert!(field.composition.is_none());
    assert!(field.undo.is_empty());
    assert!(before_window_input(
        app.world_mut(),
        &handle,
        &WindowEvent::Ime(Ime::Commit {
            window: Entity::PLACEHOLDER,
            value: "\u{306f}\u{308b}".into(),
        }),
        None,
        Modifiers::default()
    )
    .unwrap());
    assert_eq!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .value()
            .to_string(),
        "rebound"
    );
    assert!(handle.take_actions().unwrap().is_empty());
}

#[test]
fn ime_focus_loss_cancels_preedit_but_preserves_a_delivered_commit() {
    for window_blur in [false, true] {
        for committed in [false, true] {
            let (mut app, handle, entity) = editor_fixture();
            apply_edit(app.world_mut(), entity, TextEdit::SelectAll).unwrap();
            let window = Entity::PLACEHOLDER;
            before_window_input(
                app.world_mut(),
                &handle,
                &WindowEvent::Ime(Ime::Preedit {
                    window,
                    value: "\u{306f}\u{308b}".into(),
                    cursor: Some((6, 6)),
                }),
                None,
                Modifiers::default(),
            )
            .unwrap();
            if committed {
                before_window_input(
                    app.world_mut(),
                    &handle,
                    &WindowEvent::Ime(Ime::Commit {
                        window,
                        value: "\u{306f}\u{308b}".into(),
                    }),
                    None,
                    Modifiers::default(),
                )
                .unwrap();
            }
            if window_blur {
                before_window_input(
                    app.world_mut(),
                    &handle,
                    &WindowEvent::WindowFocused(bevy::window::WindowFocused {
                        window,
                        focused: false,
                    }),
                    None,
                    Modifiers::default(),
                )
                .unwrap();
            } else {
                handle.blur();
                after_window_input(app.world_mut(), &handle).unwrap();
            }
            let expected = if committed { "\u{306f}\u{308b}" } else { "12" };
            let editor = app.world().get::<EditableText>(entity).unwrap();
            assert_eq!(editor.value().to_string(), expected);
            assert!(!editor.is_composing());
            let field = app.world().get::<NativeTextField>(entity).unwrap();
            assert!(field.composition.is_none());
            assert_eq!(field.undo.len(), usize::from(committed));
            let actions = handle.take_actions().unwrap();
            assert_eq!(actions.len(), usize::from(committed));
            if committed {
                assert_eq!(
                    actions[0].control.input,
                    ControlInput::SetValue(expected.into())
                );
                history_edit(app.world_mut(), entity, false).unwrap();
                assert_eq!(
                    app.world()
                        .get::<EditableText>(entity)
                        .unwrap()
                        .value()
                        .to_string(),
                    "12"
                );
            }
        }
    }
}

#[test]
fn ime_selected_text_cancellation_restores_text_selection_and_history() {
    for disabled in [false, true] {
        let (mut app, handle, entity) = editor_fixture();
        apply_edit(app.world_mut(), entity, TextEdit::TextEnd(false)).unwrap();
        apply_edit(app.world_mut(), entity, TextEdit::TextStart(true)).unwrap();
        let selection = |world: &World| {
            let selection = world
                .get::<EditableText>(entity)
                .unwrap()
                .editor
                .raw_selection();
            (
                selection.anchor().index(),
                selection.anchor().affinity(),
                selection.focus().index(),
                selection.focus().affinity(),
            )
        };
        let before = selection(app.world());
        assert_eq!((before.0, before.2), (2, 0));
        let window = Entity::PLACEHOLDER;
        for value in ["\u{306f}", "\u{306f}\u{308b}"] {
            before_window_input(
                app.world_mut(),
                &handle,
                &WindowEvent::Ime(Ime::Preedit {
                    window,
                    value: value.into(),
                    cursor: Some((value.len(), value.len())),
                }),
                None,
                Modifiers::default(),
            )
            .unwrap();
        }
        {
            let mut editor = app.world_mut().get_mut::<EditableText>(entity).unwrap();
            editor.editor.set_scale(1.75);
            editor.viewport.size = Vec2::new(400., 40.);
        }
        let cancellation = if disabled {
            Ime::Disabled { window }
        } else {
            Ime::Preedit {
                window,
                value: String::new(),
                cursor: None,
            }
        };
        before_window_input(
            app.world_mut(),
            &handle,
            &WindowEvent::Ime(cancellation),
            None,
            Modifiers::default(),
        )
        .unwrap();
        let editor = app.world().get::<EditableText>(entity).unwrap();
        assert_eq!(editor.value().to_string(), "12", "disabled={disabled}");
        assert!(!editor.is_composing());
        assert_eq!(editor.editor.get_scale(), 1.75);
        assert_eq!(editor.viewport.size, Vec2::new(400., 40.));
        assert_eq!(selection(app.world()), before, "disabled={disabled}");
        let field = app.world().get::<NativeTextField>(entity).unwrap();
        assert!(field.undo.is_empty());
        assert!(field.redo.is_empty());
        assert!(commit_active(app.world_mut(), &handle).unwrap().is_none());
        assert!(handle.take_actions().unwrap().is_empty());
        if !disabled {
            for event in [
                Ime::Preedit {
                    window,
                    value: String::new(),
                    cursor: None,
                },
                Ime::Commit {
                    window,
                    value: String::new(),
                },
                Ime::Disabled { window },
            ] {
                before_window_input(
                    app.world_mut(),
                    &handle,
                    &WindowEvent::Ime(event),
                    None,
                    Modifiers::default(),
                )
                .unwrap();
            }
            assert_eq!(
                app.world()
                    .get::<EditableText>(entity)
                    .unwrap()
                    .value()
                    .to_string(),
                "12"
            );
            assert_eq!(selection(app.world()), before);
            assert!(app
                .world()
                .get::<NativeTextField>(entity)
                .unwrap()
                .undo
                .is_empty());
            assert!(commit_active(app.world_mut(), &handle).unwrap().is_none());
        }
    }
}

#[test]
fn ime_selected_text_replacement_undo_restores_original_text() {
    let (mut app, handle, entity) = editor_fixture();
    apply_edit(app.world_mut(), entity, TextEdit::SelectAll).unwrap();
    let window = Entity::PLACEHOLDER;
    for event in [
        Ime::Preedit {
            window,
            value: "\u{306f}\u{308b}".into(),
            cursor: Some((6, 6)),
        },
        Ime::Preedit {
            window,
            value: String::new(),
            cursor: None,
        },
        Ime::Commit {
            window,
            value: "\u{306f}\u{308b}".into(),
        },
        Ime::Disabled { window },
    ] {
        before_window_input(
            app.world_mut(),
            &handle,
            &WindowEvent::Ime(event),
            None,
            Modifiers::default(),
        )
        .unwrap();
    }
    assert_eq!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .value()
            .to_string(),
        "\u{306f}\u{308b}"
    );
    assert_eq!(
        app.world()
            .get::<NativeTextField>(entity)
            .unwrap()
            .undo
            .len(),
        1
    );
    history_edit(app.world_mut(), entity, false).unwrap();
    assert_eq!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .value()
            .to_string(),
        "12"
    );
    history_edit(app.world_mut(), entity, true).unwrap();
    assert_eq!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .value()
            .to_string(),
        "\u{306f}\u{308b}"
    );
}

#[test]
fn preedit_is_provisional_and_committing_records_one_draft_undo() {
    let (mut app, handle, entity) = editor_fixture();
    let window = Entity::PLACEHOLDER;
    let preedit = WindowEvent::Ime(Ime::Preedit {
        window,
        value: "日本".into(),
        cursor: Some((6, 6)),
    });
    before_window_input(
        app.world_mut(),
        &handle,
        &preedit,
        None,
        Modifiers::default(),
    )
    .unwrap();
    assert!(app
        .world()
        .get::<EditableText>(entity)
        .unwrap()
        .is_composing());
    assert!(app
        .world()
        .get::<NativeTextField>(entity)
        .unwrap()
        .undo
        .is_empty());
    let commit = WindowEvent::Ime(Ime::Commit {
        window,
        value: "日本".into(),
    });
    before_window_input(
        app.world_mut(),
        &handle,
        &commit,
        None,
        Modifiers::default(),
    )
    .unwrap();
    assert_eq!(
        commit_active(app.world_mut(), &handle)
            .unwrap()
            .unwrap()
            .control
            .input,
        ControlInput::SetValue("12日本".into())
    );
    history_edit(app.world_mut(), entity, false).unwrap();
    assert_eq!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .value()
            .to_string(),
        "12"
    );
}

#[test]
fn clearing_second_composition_preserves_committed_text_and_draft_history() {
    let (mut app, handle, entity) = editor_fixture();
    let window = Entity::PLACEHOLDER;
    let preedit = |value: &str| Ime::Preedit {
        window,
        value: value.into(),
        cursor: (!value.is_empty()).then_some((value.len(), value.len())),
    };
    for event in [
        Ime::Enabled { window },
        preedit("ｈ"),
        preedit("は"),
        preedit("はｒ"),
        preedit("はる"),
        preedit(""),
        Ime::Commit {
            window,
            value: "はる".into(),
        },
        Ime::Disabled { window },
        Ime::Enabled { window },
        preedit("ｈ"),
        preedit("は"),
        preedit("はｒ"),
        preedit("はる"),
        preedit("はる"),
    ] {
        assert!(before_window_input(
            app.world_mut(),
            &handle,
            &WindowEvent::Ime(event),
            None,
            Modifiers::default()
        )
        .unwrap());
    }
    assert!(app
        .world()
        .get::<EditableText>(entity)
        .unwrap()
        .is_composing());
    assert_eq!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .value()
            .to_string(),
        "12はる"
    );
    for event in [preedit(""), Ime::Disabled { window }] {
        assert!(before_window_input(
            app.world_mut(),
            &handle,
            &WindowEvent::Ime(event),
            None,
            Modifiers::default()
        )
        .unwrap());
        assert_eq!(
            app.world()
                .get::<EditableText>(entity)
                .unwrap()
                .value()
                .to_string(),
            "12はる"
        );
        assert!(!app
            .world()
            .get::<EditableText>(entity)
            .unwrap()
            .is_composing());
        assert_eq!(
            app.world()
                .get::<NativeTextField>(entity)
                .unwrap()
                .undo
                .len(),
            1
        );
        assert!(handle.take_actions().unwrap().is_empty());
    }
    assert!(before_window_input(
        app.world_mut(),
        &handle,
        &WindowEvent::Ime(Ime::Commit {
            window,
            value: "はる".into()
        }),
        None,
        Modifiers::default()
    )
    .unwrap());
    assert_eq!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .value()
            .to_string(),
        "12はるはる"
    );
    assert_eq!(
        app.world()
            .get::<NativeTextField>(entity)
            .unwrap()
            .undo
            .len(),
        2
    );
}

#[test]
fn bevy_text_viewport_keeps_pointer_selection_and_ime_on_the_visible_text() {
    let (mut app, handle, entity) = editor_fixture();
    app.init_resource::<UiScale>();
    let window = app
        .world_mut()
        .spawn((Window::default(), PrimaryWindow))
        .id();
    app.world_mut()
        .run_system_cached(bevy::ui::widget::sync_editable_text_viewports)
        .unwrap();
    apply_edit(
        app.world_mut(),
        entity,
        TextEdit::Insert("abcdefghijklmnopqrstuvwxyz".into()),
    )
    .unwrap();
    assert!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .viewport
            .offset
            .x
            > 0.
    );
    app.world_mut().run_system_cached(update_ime).unwrap();
    let window_state = app.world().get::<Window>(window).unwrap();
    assert!(window_state.ime_enabled);
    assert!((20.0..=100.0).contains(&window_state.ime_position.x));
    after_pointer_input(
        app.world_mut(),
        &handle,
        &WindowEvent::MouseButtonInput(bevy::input::mouse::MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Pressed,
            window,
        }),
        Some(Vec2::new(21., 42.)),
        Modifiers::default(),
    )
    .unwrap();
    let editor = app.world().get::<EditableText>(entity).unwrap();
    let selection = editor.editor.raw_selection().text_range();
    assert!(selection.is_empty());
    assert!(selection.start > 0 && selection.start < editor.value().to_string().len());
}

#[test]
fn dpi_change_recomputes_the_candidate_popup_without_dropping_composition() {
    let (mut app, handle, entity) = editor_fixture();
    app.init_resource::<UiScale>();
    app.init_resource::<super::ime_popup::ImeCandidateWindow>();
    let window = app
        .world_mut()
        .spawn((Window::default(), PrimaryWindow))
        .id();
    let value = "\u{306f}\u{308b}";
    assert!(before_window_input(
        app.world_mut(),
        &handle,
        &WindowEvent::Ime(Ime::Preedit {
            window,
            value: value.into(),
            cursor: Some((value.len(), value.len())),
        }),
        None,
        Modifiers::default(),
    )
    .unwrap());
    assert!(app
        .world()
        .get::<EditableText>(entity)
        .unwrap()
        .is_composing());
    assert!(app
        .world()
        .get::<NativeTextField>(entity)
        .unwrap()
        .composition
        .is_some());
    let provisional = app
        .world()
        .get::<EditableText>(entity)
        .unwrap()
        .editor
        .raw_text()
        .to_owned();
    let committed = app
        .world()
        .get::<EditableText>(entity)
        .unwrap()
        .value()
        .to_string();
    assert!(provisional.contains(value));
    assert_eq!(committed, "12");
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set_scale_factor(2.);
    {
        let mut transform = app
            .world_mut()
            .get_mut::<UiGlobalTransform>(entity)
            .unwrap();
        *transform = UiGlobalTransform::from_translation(Vec2::new(90., 50.));
        let mut editor = app.world_mut().get_mut::<EditableText>(entity).unwrap();
        editor.viewport.offset = Vec2::new(12., 3.);
    }
    app.world_mut().run_system_cached(update_ime).unwrap();
    let editor = app.world().get::<EditableText>(entity).unwrap();
    assert!(editor.is_composing());
    assert_eq!(editor.editor.raw_text(), provisional);
    assert_eq!(editor.value().to_string(), committed);
    assert!(app
        .world()
        .get::<NativeTextField>(entity)
        .unwrap()
        .composition
        .is_some());
    let area = editor.editor.ime_cursor_area();
    let node = app.world().get::<ComputedNode>(entity).unwrap();
    let transform = app.world().get::<UiGlobalTransform>(entity).unwrap();
    let expected = super::ime_popup::place_ime_popup(super::ime_popup::ImePopupInput {
        caret: super::ime_popup::PixelRect {
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
        ui_scale: 1.,
        monitor_scale: 2.,
    })
    .unwrap();
    let state = app
        .world()
        .resource::<super::ime_popup::ImeCandidateWindow>();
    assert_eq!(state.popup, Some(expected.popup));
    assert_eq!(state.field_pixels, Some(expected.field_pixels));
    assert_eq!(state.scale_factor, 2.);
    assert_eq!(state.field_pixels.unwrap().width, node.size().x * 2.);
    assert_eq!(
        app.world().get::<Window>(window).unwrap().ime_position,
        Vec2::new(expected.popup.origin[0], expected.popup.origin[1])
    );
    assert!(app.world().get::<Window>(window).unwrap().ime_enabled);
}

#[test]
fn live_feature_numbers_publish_each_keyboard_edit_and_wait_for_ime_commit() {
    let (mut app, handle, entity) = editor_fixture_with_submit(true);
    app.world_mut().entity_mut(entity).insert(LiveValue);
    apply_edit(app.world_mut(), entity, TextEdit::SelectAll).unwrap();
    for (input, expected) in [("2", "2"), ("5", "25")] {
        assert!(before_window_input(
            app.world_mut(),
            &handle,
            &key(Key::Character(input.into()), Some(input)),
            None,
            default()
        )
        .unwrap());
        let actions = handle.take_actions().unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0].control.input,
            ControlInput::SetValue(expected.into())
        );
        acknowledge_control_input(app.world_mut(), &actions[0], true);
    }
    assert!(before_window_input(
        app.world_mut(),
        &handle,
        &WindowEvent::Ime(Ime::Preedit {
            window: Entity::PLACEHOLDER,
            value: "8".into(),
            cursor: Some((1, 1))
        }),
        None,
        default()
    )
    .unwrap());
    assert!(
        handle.take_actions().unwrap().is_empty(),
        "Composition is not an accepted numeric draft"
    );
    assert!(before_window_input(
        app.world_mut(),
        &handle,
        &WindowEvent::Ime(Ime::Commit {
            window: Entity::PLACEHOLDER,
            value: "8".into()
        }),
        None,
        default()
    )
    .unwrap());
    let actions = handle.take_actions().unwrap();
    assert_eq!(actions.len(), 1);
    assert!(
        matches!(&actions[0].control.input, ControlInput::SetValue(value) if value.contains('8'))
    );
}
