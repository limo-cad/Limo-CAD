use super::*;
use crate::native_viewport::interface_shell::{PointerButton, PointerPhase};
use limo_cad_interface::ControlKey;

fn paint_order(world: &mut World) -> Vec<Entity> {
    use bevy::ui::experimental::{UiChildren, UiRootNodes};

    fn visit(entity: Entity, children: &HashMap<Entity, Vec<Entity>>, order: &mut Vec<Entity>) {
        order.push(entity);
        if let Some(entries) = children.get(&entity) {
            for child in entries {
                visit(*child, children, order);
            }
        }
    }

    // The pinned stack system assigns a unique rank to each UI node, traverses
    // local-Z siblings, and treats GlobalZ nodes as separate stack roots.
    // It does not promise a fixed order for newly created equal-Z roots.
    let nodes: Vec<_> = world
        .query_filtered::<Entity, With<Node>>()
        .iter(world)
        .collect();
    let mut params = SystemState::<(UiRootNodes, UiChildren)>::new(world);
    let (ui_roots, ui_children) = params.get(world).unwrap();
    let mut roots: Vec<_> = ui_roots.iter().collect();
    let mut children = HashMap::new();
    for entity in &nodes {
        if world.get::<bevy::ui::GlobalZIndex>(*entity).is_some() && !roots.contains(entity) {
            roots.push(*entity);
        }
        let mut entries: Vec<_> = ui_children
            .iter_ui_children(*entity)
            .filter(|child| {
                world.get::<bevy::ui::ComputedStackIndex>(*child).is_some()
                    && world.get::<bevy::ui::GlobalZIndex>(*child).is_none()
            })
            .collect();
        entries.sort_by_key(|child| world.get::<ZIndex>(*child).map_or(0, |z| z.0));
        children.insert(*entity, entries);
    }
    roots.sort_by_key(|entity| {
        (
            world
                .get::<bevy::ui::GlobalZIndex>(*entity)
                .map_or(0, |z| z.0),
            world.get::<ZIndex>(*entity).map_or(0, |z| z.0),
        )
    });
    let mut order = Vec::new();
    for root in roots {
        visit(root, &children, &mut order);
    }
    order
}

fn absolute_layout(world: &World, entity: Entity) -> (Vec2, Vec2, bool) {
    let mut current = Some(entity);
    let mut origin = Vec2::ZERO;
    let mut displayed = true;
    let mut visible = None;
    let mut size = Vec2::ZERO;
    while let Some(parent) = current {
        if let Some(node) = world.get::<Node>(parent) {
            displayed &=
                node.display != Display::None && node.position_type == PositionType::Absolute;
            match (node.left, node.top) {
                (Val::Px(x), Val::Px(y)) => origin += Vec2::new(x, y),
                _ => displayed = false, // Flow/right-anchored layout is not supplied by this adapter.
            }
            if parent == entity {
                if let (Val::Px(width), Val::Px(height)) = (node.width, node.height) {
                    size = Vec2::new(width.max(0.), height.max(0.));
                }
            }
        }
        if visible.is_none() {
            visible = match world.get::<Visibility>(parent) {
                Some(Visibility::Visible) => Some(true),
                Some(Visibility::Hidden) => Some(false),
                _ => None,
            };
        }
        current = world.get::<ChildOf>(parent).map(ChildOf::parent);
    }
    let visible = displayed && visible.unwrap_or(true);
    if !visible {
        size = Vec2::ZERO;
    }
    (size, origin + size / 2., visible)
}

fn publish(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    state: &mut Controller,
) {
    synchronize(world, handle, services, state).unwrap();
    // Supply a bounded headless absolute-layout/paint-order adapter for the
    // production-generated UI. The established publication hook exercises
    // hit/occlusion and authorization; this does not qualify native GPU layout
    // or platform pointer behavior.
    for (index, entity) in paint_order(world).into_iter().enumerate() {
        let (size, center, visible) = absolute_layout(world, entity);
        world.entity_mut(entity).insert((
            ComputedNode {
                size,
                inverse_scale_factor: 1.,
                ..default()
            },
            bevy::ui::UiGlobalTransform::from_translation(center),
            bevy::ui::ComputedStackIndex(index.try_into().unwrap()),
            if visible {
                InheritedVisibility::VISIBLE
            } else {
                InheritedVisibility::HIDDEN
            },
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
    let (target, modal) = handle
        .read_surface(|_, frame| {
            (
                frame
                    .controls
                    .iter()
                    .find(|c| c.key == key)
                    .unwrap()
                    .clone(),
                frame.modal_stack.clone(),
            )
        })
        .unwrap();
    assert_eq!(
        handle.hit_key(point),
        Some(key),
        "target {target:?}, point {point:?}, modal {modal:?}"
    );
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
    let file_expanded = || {
        handle
            .read_surface(|_, frame| {
                frame
                    .controls
                    .iter()
                    .find(|c| c.key == file)
                    .unwrap()
                    .expanded
            })
            .unwrap()
    };
    let click = |world: &mut World, key| {
        let action = pointer_action(&handle, key);
        reduce_control_input(&services.engine, &services.bridge, world, &handle, &action).unwrap()
    };

    click(world, build);
    publish(world, &handle, &services, &mut state);
    assert_eq!(handle.frame().unwrap().modal_stack, vec!["workbench-menu"]);
    assert_eq!(file_expanded(), Some(false));
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
    assert_eq!(file_expanded(), Some(true));
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
                .clone()
        })
        .unwrap();
    assert!(covered.visible && !covered.disabled);
    assert!(covered.bounds.width > 0. && covered.bounds.height > 0.);
    assert!(handle.resolve_retained(workspace).is_ok());
    assert_ne!(
        handle.hit_key([
            covered.bounds.x + covered.bounds.width / 2.,
            covered.bounds.y + covered.bounds.height / 2.
        ]),
        Some(workspace),
        "File's card/entries must still cover the workspace anchor"
    );
    assert_eq!(handle.frame().unwrap().modal_stack, vec!["file-menu"]);
    assert!(reduce_control_input(
        &services.engine,
        &services.bridge,
        world,
        &handle,
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
        &services.engine,
        &services.bridge,
        world,
        &handle,
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
    assert_eq!(file_expanded(), Some(false));
    assert_eq!(workbench::modal(world), None);

    click(world, build);
    publish(world, &handle, &services, &mut state);
    let dismiss = control(
        world,
        &NativeCommand::Workbench(workbench::Command::Dismiss),
    );
    let client = handle.frame().unwrap().client;
    let outside = pointer_action_at(&handle, dismiss, [client.width - 4., client.height - 4.]);
    reduce_control_input(&services.engine, &services.bridge, world, &handle, &outside).unwrap();
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
        &services.engine,
        &services.bridge,
        world,
        &handle,
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
