//! A focused native buffer must be committed before the first Apply click.
use super::super::*;
use super::document;
use crate::native_viewport::{
    interface_shell::fields,
    winit_host::{prepare_native_input, HostInputState, Modifiers, NativeHostInput},
};
use crate::session_bridge::native_interface::{controller::reduce_control_input, tests::Fixture};
use bevy::{
    input::{
        keyboard::{Key, KeyCode, KeyboardInput, NativeKeyCode},
        mouse::MouseButtonInput,
        ButtonState,
    },
    text::EditableText,
    ui::{ComputedStackIndex, ComputedUiRenderTargetInfo, UiGlobalTransform, UiScale},
};
use limo_cad_interface::ControlKey;

fn key(value: &str, command: bool, redo: bool) -> (WindowEvent, Modifiers) {
    (
        WindowEvent::KeyboardInput(KeyboardInput {
            key_code: KeyCode::Unidentified(NativeKeyCode::Unidentified),
            logical_key: Key::Character(value.into()),
            text: (!command).then(|| value.into()),
            state: ButtonState::Pressed,
            repeat: false,
            window: Entity::PLACEHOLDER,
        }),
        Modifiers {
            ctrl: command && !cfg!(target_os = "macos"),
            meta: command && cfg!(target_os = "macos"),
            shift: redo,
            ..default()
        },
    )
}

fn dispatch(
    app: &mut App,
    handle: &NativeInterfaceHandle,
    (event, modifiers): (WindowEvent, Modifiers),
    cursor: Option<Vec2>,
) -> NativeHostInput {
    let mut input = NativeHostInput {
        ui_scale: 1.,
        context: handle.presented_context(),
        cursor,
        modifiers,
        event,
        consumed: false,
        actions: vec![],
    };
    prepare_native_input(app.world_mut(), handle, &mut input).unwrap();
    assert!(input.consumed);
    input
}

fn layout(world: &mut World, entity: Entity) -> Vec2 {
    let node = world.get::<Node>(entity).unwrap();
    let (Val::Px(x), Val::Px(y), Val::Px(width), Val::Px(height)) =
        (node.left, node.top, node.width, node.height)
    else {
        panic!("The drawing panel uses absolute pixel bounds")
    };
    let center = Vec2::new(x + width / 2., y + height / 2.);
    world.entity_mut(entity).insert((
        ComputedNode {
            size: Vec2::new(width, height),
            inverse_scale_factor: 1.,
            ..default()
        },
        UiGlobalTransform::from_translation(center),
        ComputedStackIndex(3),
        ComputedUiRenderTargetInfo::default(),
        InheritedVisibility::VISIBLE,
    ));
    center
}

#[test]
fn focused_drawing_title_enables_apply_and_commits_on_the_first_native_click() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let before = document();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "drawing_set_document",
            &serde_json::to_value(&before).unwrap(),
            || Ok(()),
        )
        .unwrap();
    let owner = fixture.owner();
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &owner)
        .unwrap();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let handle = NativeInterfaceHandle::new(|| {});
    let mut app = native_viewport::interface_scene_fixture();
    // Retain the production scene resources for the guarded drawing mutation,
    // but run only text/field/layout systems in this input-lifecycle fixture.
    app.add_schedule(Schedule::new(Startup))
        .add_schedule(Schedule::new(Update))
        .add_schedule(Schedule::new(Last))
        .add_plugins(bevy::text::TextPlugin)
        .init_resource::<Assets<Image>>()
        .init_resource::<Time<bevy::time::Real>>()
        .init_resource::<UiScale>()
        .init_resource::<bevy::input_focus::InputFocus>()
        .init_resource::<HostInputState>()
        .insert_resource(handle.clone())
        .insert_resource(Workbench {
            workspace: Workspace::Drawing,
            ..default()
        });
    fields::install(&mut app);
    let bounds = InterfaceRect {
        x: 0.,
        y: 0.,
        width: 1000.,
        height: 860.,
    };
    handle
        .present(InterfaceFrame {
            context: owner.clone(),
            client: bounds,
            surface: bounds,
            canvases: vec![],
            surfaces: vec![Surface {
                name: "drawing/document".into(),
                text: None,
            }],
            modal_stack: vec![],
            document_visible: true,
        })
        .unwrap();
    let camera = app.world_mut().spawn(InterfaceCamera).id();
    let paint = |app: &mut App| {
        synchronize(
            app.world_mut(),
            camera,
            &services,
            &owner,
            (860., 248.),
            true,
            &Workbench::default(),
        )
        .unwrap();
        app.update();
        interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
    };
    paint(&mut app);
    let editor = app.world().resource::<Editor>();
    let title = editor
        .widgets
        .entity("drawing-field-/title_block/title")
        .unwrap();
    let apply = editor.widgets.entity("drawing-apply").unwrap();
    layout(app.world_mut(), title);
    let point = layout(app.world_mut(), apply);
    paint(&mut app);
    app.world_mut()
        .run_system_cached(bevy::ui::widget::update_editable_text_styles)
        .unwrap();
    app.world_mut()
        .run_system_cached(bevy::ui::widget::update_editable_text_layout)
        .unwrap();
    let focus = handle
        .resolve_retained(ControlKey(title.to_bits()))
        .unwrap();
    reduce_control_input(
        &fixture.engine,
        &fixture.bridge,
        app.world_mut(),
        &handle,
        &focus,
    )
    .unwrap();
    assert!(app.world().get::<InterfaceControl>(apply).unwrap().disabled);
    assert!(dispatch(&mut app, &handle, key("a", true, false), None)
        .actions
        .is_empty());
    let changed = "X";
    assert!(
        dispatch(&mut app, &handle, key(changed, false, false), None)
            .actions
            .is_empty()
    );
    paint(&mut app);
    assert_eq!(
        app.world().get::<EditableText>(title).unwrap().value(),
        changed
    );
    assert_eq!(handle.focused_key(), Some(ControlKey(title.to_bits())));
    assert!(!app
        .world()
        .resource::<Editor>()
        .draft
        .as_ref()
        .unwrap()
        .dirty());
    assert_eq!(fixture.engine.drawing_snapshot(), before);
    assert!(!app.world().get::<InterfaceControl>(apply).unwrap().disabled);

    // Local Undo/Redo toggles Apply without submitting any drawing mutation.
    assert!(dispatch(&mut app, &handle, key("z", true, false), None)
        .actions
        .is_empty());
    paint(&mut app);
    assert_eq!(
        app.world().get::<EditableText>(title).unwrap().value(),
        "Saved title"
    );
    assert!(app.world().get::<InterfaceControl>(apply).unwrap().disabled);
    assert!(dispatch(&mut app, &handle, key("z", true, true), None)
        .actions
        .is_empty());
    paint(&mut app);
    assert_eq!(
        app.world().get::<EditableText>(title).unwrap().value(),
        changed
    );
    assert_eq!(fixture.engine.drawing_snapshot(), before);
    assert!(!app.world().get::<InterfaceControl>(apply).unwrap().disabled);
    let click = |state| {
        (
            WindowEvent::MouseButtonInput(MouseButtonInput {
                button: MouseButton::Left,
                state,
                window: Entity::PLACEHOLDER,
            }),
            Modifiers::default(),
        )
    };
    let press = dispatch(&mut app, &handle, click(ButtonState::Pressed), Some(point));
    assert_eq!(press.actions.len(), 1);
    assert_eq!(press.actions[0].control.key, ControlKey(title.to_bits()));
    assert_eq!(
        press.actions[0].control.input,
        ControlInput::SetValue(changed.into())
    );
    assert_eq!(fixture.engine.drawing_snapshot(), before);
    reduce_control_input(
        &fixture.engine,
        &fixture.bridge,
        app.world_mut(),
        &handle,
        &press.actions[0],
    )
    .unwrap();
    paint(&mut app);
    let release = dispatch(&mut app, &handle, click(ButtonState::Released), Some(point));
    assert_eq!(release.actions.len(), 1);
    assert_eq!(release.actions[0].control.key, ControlKey(apply.to_bits()));
    assert_eq!(release.actions[0].control.input, ControlInput::Click);
    reduce_control_input(
        &fixture.engine,
        &fixture.bridge,
        app.world_mut(),
        &handle,
        &release.actions[0],
    )
    .unwrap();
    let mut expected = before;
    expected.sheets[0].title_block.title = changed.into();
    assert_eq!(fixture.engine.drawing_snapshot(), expected);
    assert_eq!(
        fixture
            .bridge
            .native_document_receipt(&fixture.engine, &owner)
            .unwrap()
            .revision,
        receipt.revision + 1,
        "The one click must commit exactly one drawing mutation"
    );
    paint(&mut app);
    assert!(!app
        .world()
        .resource::<Editor>()
        .draft
        .as_ref()
        .unwrap()
        .dirty());
    assert!(app.world().get::<InterfaceControl>(apply).unwrap().disabled);
}
