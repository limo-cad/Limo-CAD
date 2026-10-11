use super::*;
use crate::native_viewport::interface_shell::{PointerButton, PointerPhase};
use limo_cad_interface::ControlKey;

fn publish(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    state: &mut Controller,
) {
    synchronize(world, handle, services, state).unwrap();
    // Use the production-generated absolute UI bounds with the established
    // headless layout adapter. This exercises hit/occlusion and authorization,
    // without qualifying native GPU rendering or platform pointer behavior.
    let nodes: Vec<_> = world
        .query::<(Entity, &Node, Option<&ZIndex>)>()
        .iter(world)
        .filter_map(|(entity, node, z)| {
            let (Val::Px(x), Val::Px(y), Val::Px(width), Val::Px(height)) =
                (node.left, node.top, node.width, node.height)
            else {
                return None;
            };
            (width > 0. && height > 0.).then_some((
                entity,
                Vec2::new(width, height),
                Vec2::new(x + width / 2., y + height / 2.),
                z.map_or(0, |z| z.0),
            ))
        })
        .collect();
    let minimum_z = nodes.iter().map(|(_, _, _, z)| *z).min().unwrap_or(0);
    for (entity, size, center, z) in nodes {
        world.entity_mut(entity).insert((
            ComputedNode {
                size,
                inverse_scale_factor: 1.,
                ..default()
            },
            bevy::ui::UiGlobalTransform::from_translation(center),
            bevy::ui::ComputedStackIndex((i64::from(z) - i64::from(minimum_z)).try_into().unwrap()),
            InheritedVisibility::VISIBLE,
        ));
    }
    interface_shell::tests::publish_layout_once(world, handle.clone());
}

fn control(world: &mut World, command: &NativeCommand) -> ControlKey {
    let entity = world
        .query::<(Entity, &NativeCommandBinding, &InterfaceControl)>()
        .iter(world)
        .find(|(_, binding, control)| {
            &binding.command == command && control.visible && !control.disabled
        })
        .unwrap_or_else(|| panic!("missing menu control: {command:?}"))
        .0;
    ControlKey(entity.to_bits())
}

fn pointer_action(handle: &NativeInterfaceHandle, key: ControlKey) -> NativeInterfaceAction {
    let bounds = handle
        .read_surface(|_, frame| frame.controls.iter().find(|c| c.key == key).unwrap().bounds)
        .unwrap();
    let center = [bounds.x + bounds.width / 2., bounds.y + bounds.height / 2.];
    pointer_action_at(handle, key, center)
}

fn pointer_action_at(
    handle: &NativeInterfaceHandle,
    key: ControlKey,
    point: [f64; 2],
) -> NativeInterfaceAction {
    assert_eq!(handle.hit_key(point), Some(key));
    assert!(handle
        .pointer(PointerPhase::Down, point, PointerButton::Primary)
        .unwrap());
    assert!(handle
        .pointer(PointerPhase::Up, point, PointerButton::Primary)
        .unwrap());
    let action = handle.take_next_action().unwrap().unwrap();
    assert_eq!(action.control.key, key);
    assert!(handle.take_next_action().unwrap().is_none());
    action
}

#[test]
fn sibling_menu_clicks_transfer_modal_ownership_and_keep_dialog_and_stale_action_guards() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let before = serde_json::to_string(&fixture.engine.document_snapshot()).unwrap();
    let revision = services
        .bridge
        .native_document_receipt(&services.engine, &fixture.owner())
        .unwrap();
    let mut app = native_viewport::interface_scene_fixture();
    app.insert_resource(ViewportUiAssets::default());
    app.insert_resource(services.clone());
    app.world_mut().spawn((Window::default(), PrimaryWindow));
    app.world_mut().spawn(InterfaceCamera);
    let handle = NativeInterfaceHandle::new(|| {});
    let mut state = Controller::new("main".into(), None, Arc::new(AtomicBool::new(false)));
    let world = app.world_mut();
    publish(world, &handle, &services, &mut state);
    let build_command = NativeCommand::Workbench(workbench::Command::Menu("build".into()));
    let refine_command = NativeCommand::Workbench(workbench::Command::Menu("refine".into()));
    let file_command = NativeCommand::File(files::FileCommand::Menu);
    let build = control(world, &build_command);
    let refine = control(world, &refine_command);
    let file = control(world, &file_command);
    let click = |world: &mut World, key| {
        let action = pointer_action(&handle, key);
        reduce_control_input(world, &handle, &services.engine, &services.bridge, &action).unwrap()
    };

    click(world, build);
    publish(world, &handle, &services, &mut state);
    assert_eq!(handle.frame().unwrap().modal_stack, vec!["workbench-menu"]);
    let old_build_row = handle
        .read_surface(|_, frame| {
            frame
                .controls
                .iter()
                .find(|c| c.role == "menuitem" && c.visible && !c.disabled)
                .unwrap()
                .key
        })
        .unwrap();
    let old_build_row = handle.resolve_retained(old_build_row).unwrap();
    click(world, file);
    publish(world, &handle, &services, &mut state);
    assert_eq!(handle.frame().unwrap().modal_stack, vec!["file-menu"]);
    assert_eq!(workbench::modal(world), None);
    let workspace = control(
        world,
        &NativeCommand::Workbench(workbench::Command::Menu("workspace".into())),
    );
    let covered = handle
        .read_surface(|_, frame| {
            frame
                .controls
                .iter()
                .find(|c| c.key == workspace)
                .unwrap()
                .bounds
        })
        .unwrap();
    assert_ne!(
        handle.hit_key([
            covered.x + covered.width / 2.,
            covered.y + covered.height / 2.
        ]),
        Some(workspace),
        "File's card/entries must still cover the workspace anchor"
    );
    assert_eq!(handle.frame().unwrap().modal_stack, vec!["file-menu"]);
    assert!(reduce_control_input(
        world,
        &handle,
        &services.engine,
        &services.bridge,
        &old_build_row
    )
    .is_err());
    let rename = control(world, &NativeCommand::File(files::FileCommand::Rename));
    let old_file_row = handle.resolve_retained(rename).unwrap();

    // REFINE is visible outside File's dropdown card. Its one click must win
    // over the outside-dismissal backdrop, while covered anchors stay covered.
    click(world, refine);
    publish(world, &handle, &services, &mut state);
    assert_eq!(handle.frame().unwrap().modal_stack, vec!["workbench-menu"]);
    assert_eq!(files::modal(world), None);
    assert!(reduce_control_input(
        world,
        &handle,
        &services.engine,
        &services.bridge,
        &old_file_row
    )
    .is_err());
    click(world, build);
    publish(world, &handle, &services, &mut state);
    assert!(handle
        .read_surface(|_, frame| frame
            .controls
            .iter()
            .find(|c| c.key == build)
            .unwrap()
            .expanded)
        .unwrap()
        .unwrap());
    click(world, file);
    publish(world, &handle, &services, &mut state);
    click(world, file);
    publish(world, &handle, &services, &mut state);
    assert!(handle.frame().unwrap().modal_stack.is_empty());
    assert_eq!(workbench::modal(world), None);

    click(world, build);
    publish(world, &handle, &services, &mut state);
    let dismiss = control(
        world,
        &NativeCommand::Workbench(workbench::Command::Dismiss),
    );
    let client = handle.frame().unwrap().client;
    let outside = pointer_action_at(&handle, dismiss, [client.width - 4., client.height - 4.]);
    reduce_control_input(world, &handle, &services.engine, &services.bridge, &outside).unwrap();
    publish(world, &handle, &services, &mut state);
    assert!(handle.frame().unwrap().modal_stack.is_empty());
    click(world, file);
    publish(world, &handle, &services, &mut state);
    assert!(handle
        .key(limo_cad_interface::KeyChord::plain("Escape"))
        .unwrap());
    process_modal_keys(
        world,
        &handle,
        &services.bridge,
        &services.engine,
        &mut state,
    )
    .unwrap();
    publish(world, &handle, &services, &mut state);
    assert!(handle.frame().unwrap().modal_stack.is_empty());

    click(world, file);
    publish(world, &handle, &services, &mut state);
    let stale_file_anchor = handle.resolve_retained(file).unwrap();
    let rename = control(world, &NativeCommand::File(files::FileCommand::Rename));
    click(world, rename);
    publish(world, &handle, &services, &mut state);
    assert_eq!(handle.frame().unwrap().modal_stack, vec!["file-dialog"]);
    for key in [build, file] {
        assert!(handle.resolve_retained(key).is_err());
    }
    assert!(reduce_control_input(
        world,
        &handle,
        &services.engine,
        &services.bridge,
        &stale_file_anchor
    )
    .is_err());
    assert_eq!(files::modal(world), Some("file-dialog"));
    assert_eq!(
        serde_json::to_string(&fixture.engine.document_snapshot()).unwrap(),
        before
    );
    assert_eq!(
        services
            .bridge
            .native_document_receipt(&services.engine, &fixture.owner())
            .unwrap(),
        revision
    );
}
