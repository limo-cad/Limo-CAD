//! The OS close branch runs before blur routing. Pending text stays disposable.
use super::*;
use crate::native_viewport::{
    interface_shell::{fields, tests::fixture as layout_fixture},
    winit_host::{prepare_native_input, HostInputState, Modifiers},
};
use bevy::{
    input::{
        keyboard::{Key, KeyCode, KeyboardInput, NativeKeyCode},
        ButtonState,
    },
    text::EditableText,
    ui::{ComputedStackIndex, ComputedUiRenderTargetInfo, UiGlobalTransform, UiScale},
};
use limo_cad_interface::ControlKey;

fn key(key: Key, text: Option<&str>) -> WindowEvent {
    WindowEvent::KeyboardInput(KeyboardInput {
        key_code: KeyCode::Unidentified(NativeKeyCode::Unidentified),
        logical_key: key,
        text: text.map(Into::into),
        state: ButtonState::Pressed,
        repeat: false,
        window: Entity::PLACEHOLDER,
    })
}

fn dispatch(app: &mut App, handle: &NativeInterfaceHandle, event: WindowEvent, ctrl: bool) {
    let mut input = NativeHostInput {
        ui_scale: 1.,
        context: handle.presented_context(),
        cursor: None,
        modifiers: Modifiers {
            ctrl: ctrl && !cfg!(target_os = "macos"),
            meta: ctrl && cfg!(target_os = "macos"),
            ..default()
        },
        event,
        consumed: false,
        actions: vec![],
    };
    prepare_native_input(app.world_mut(), handle, &mut input).unwrap();
    assert!(input.consumed);
    assert!(input.actions.is_empty(), "Typing is not a form commit");
}

#[test]
fn native_close_before_text_blur_preserves_the_pending_value_and_allows_undo_recovery() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let model = Fixture::new();
    let owner = model.owner();
    let receipt = model
        .bridge
        .native_document_receipt(&model.engine, &owner)
        .unwrap();
    let before = model.engine.engine_call("project_export_model", "");
    let (mut app, handle, _, _) = layout_fixture();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::text::TextPlugin,
    ))
    .init_resource::<bevy::input_focus::InputFocus>()
    .init_resource::<UiScale>()
    .init_resource::<ViewportUiAssets>()
    .init_resource::<Assets<Image>>()
    .init_resource::<HostInputState>();
    fields::install(&mut app);
    let mut frame = handle.frame().unwrap();
    frame.context = owner.clone();
    handle.present(frame).unwrap();
    let camera = app.world_mut().spawn_empty().id();
    let mut control = InterfaceControl::button("Viewport", "Pending name");
    control.field = limo_cad_interface::Field::Text {
        value: "12".into(),
        read_only: false,
        selection: None,
    };
    let entity = fields::spawn_text_field(
        &mut app.world_mut().commands(),
        camera,
        Node::default(),
        control,
        crate::native_viewport::ui::ViewportUiTheme::from_palette(&default()),
        &ViewportUiAssets::default(),
    )
    .unwrap();
    app.world_mut().flush();
    bind_command(
        app.world_mut(),
        entity,
        NativeCommand::File(files::FileCommand::Name(0)),
    )
    .unwrap();
    app.world_mut().entity_mut(entity).insert((
        ComputedNode {
            size: Vec2::new(100., 28.),
            inverse_scale_factor: 1.,
            ..default()
        },
        UiGlobalTransform::from_translation(Vec2::new(250., 180.)),
        ComputedStackIndex(2),
        ComputedUiRenderTargetInfo::default(),
        InheritedVisibility::VISIBLE,
    ));
    app.update();
    app.world_mut()
        .run_system_cached(bevy::ui::widget::update_editable_text_styles)
        .unwrap();
    app.world_mut()
        .run_system_cached(bevy::ui::widget::update_editable_text_layout)
        .unwrap();
    let action = handle
        .resolve_retained(ControlKey(entity.to_bits()))
        .unwrap();
    handle.prepare_activation(&action).unwrap();
    fields::after_window_input(app.world_mut(), &handle).unwrap();
    let mut workspace = workspace::DocumentWorkspace::default();
    workspace
        .observe(&model.bridge, &model.engine, "main")
        .unwrap();
    let workspace = Arc::new(std::sync::Mutex::new(workspace));
    files::initialize(app.world_mut(), Arc::clone(&workspace));
    let mut controller = Controller::new("main".into(), None, Arc::new(AtomicBool::new(false)));
    controller.workspace = workspace;
    assert!(fields::guard_document_close(app.world(), &owner).is_ok());
    assert!(inbox_model_mutation_reject_reason(app.world(), &owner).is_none());

    dispatch(
        &mut app,
        &handle,
        key(Key::Character("a".into()), None),
        true,
    );
    dispatch(
        &mut app,
        &handle,
        key(Key::Character("3".into()), Some("3")),
        false,
    );
    assert_eq!(
        app.world().get::<EditableText>(entity).unwrap().value(),
        "3"
    );
    assert!(inbox_model_mutation_reject_reason(app.world(), &owner)
        .unwrap()
        .contains("Finish the active text edit"));
    let close = NativeHostInput {
        ui_scale: 1.,
        context: Some(owner.clone()),
        cursor: None,
        modifiers: Modifiers::default(),
        event: WindowEvent::WindowCloseRequested(bevy::window::WindowCloseRequested {
            window: Entity::PLACEHOLDER,
        }),
        consumed: false,
        actions: vec![],
    };
    // Match the controller's early OS-close branch: do not manufacture a blur
    // or submit the field before it checks the current document's close guard.
    let error = close_from_window_event(
        app.world_mut(),
        &mut controller,
        &model.bridge,
        &model.engine,
        close.context.as_ref(),
    )
    .unwrap_err();
    assert!(error.contains("Finish the active text edit"), "{error}");
    assert!(!controller.close_pending && !controller.exit_after_receipt);
    assert_eq!(
        app.world().get::<EditableText>(entity).unwrap().value(),
        "3"
    );
    assert_eq!(handle.focused_key(), Some(ControlKey(entity.to_bits())));
    assert_eq!(model.engine.engine_call("project_export_model", ""), before);
    assert_eq!(
        model
            .bridge
            .native_document_receipt(&model.engine, &owner)
            .unwrap(),
        receipt
    );
    for field in 0..3 {
        let mut foreign = owner.clone();
        match field {
            0 => foreign.window_id.push_str("-other"),
            1 => foreign.document_id.push_str("-other"),
            _ => foreign.epoch += 1,
        }
        assert!(fields::guard_document_close(app.world(), &foreign).is_ok());
        assert!(inbox_model_mutation_reject_reason(app.world(), &foreign).is_none());
    }

    // Native local Undo is an explicit way to cancel this text change. It
    // restores the baseline without committing or mutating the CAD document.
    dispatch(
        &mut app,
        &handle,
        key(Key::Character("z".into()), None),
        true,
    );
    assert_eq!(
        app.world().get::<EditableText>(entity).unwrap().value(),
        "12"
    );
    assert!(fields::guard_document_close(app.world(), &owner).is_ok());
    assert!(inbox_model_mutation_reject_reason(app.world(), &owner).is_none());
    close_from_window_event(
        app.world_mut(),
        &mut controller,
        &model.bridge,
        &model.engine,
        close.context.as_ref(),
    )
    .unwrap();
    assert!(controller.close_pending || controller.exit_after_receipt);
    assert_eq!(model.engine.engine_call("project_export_model", ""), before);
}
